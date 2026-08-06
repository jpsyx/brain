//! Invoke `rclone` (thin `Command` shell) and parse its summary into a typed
//! `RunOutcome`. Only the parser is unit-tested; the process spawn is a thin
//! shell exercised via the integration path.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::progress;

/// rclone's bisync working directory, owned by brain.
///
/// Its location is deterministic rather than HOME-dependent, and stale lock
/// files are reapable. Stored below the selected workspace's sync cache.
#[must_use]
pub fn bisync_workdir(paths: &crate::workspace::WorkspacePaths) -> PathBuf {
    paths.sync_dir().join("bisync")
}

/// Remove leftover rclone bisync lock files from an interrupted run.
///
/// Safe to call unconditionally *while brain's own sync lock is held*: brain
/// serializes all syncs, so any `.lck` present is necessarily from a dead run
/// that was killed before it could clean up (TUI quit, power off). Baseline
/// `.lst` listings are left intact so a normal run can still resume. Missing
/// workdir or unreadable entries degrade to a no-op.
pub fn reap_stale_bisync_locks(workdir: &Path) {
    let Ok(entries) = std::fs::read_dir(workdir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "lck") {
            crate::logging::log(format!("reap stale rclone bisync lock {}", path.display()));
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// Why a bisync aborted, when it did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbortKind {
    /// `--max-delete` guard tripped (rclone's "too many deletes" safety abort).
    MaxDelete,
    /// rclone's `--check-access` marker guard failed.
    CheckAccess,
    /// Baseline listings missing — needs `brain sync repair`.
    PriorListingMissing,
    /// Some other non-zero exit.
    Other,
}

/// Parsed result of one rclone run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    pub exit_ok: bool,
    pub transferred: u64,
    pub deleted: u64,
    pub errors: u64,
    pub abort: Option<AbortKind>,
}

/// Return whether the external rclone executable can be launched.
#[must_use]
pub fn rclone_present() -> bool {
    Command::new("rclone")
        .arg("version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

/// Explain how to install the external rclone transport when it is missing.
#[must_use]
pub fn missing_rclone_guidance(theme: crate::theme::Theme, retry_command: &str) -> String {
    format!(
        "{}\n\n{}\n\n{}\n  {}\n\n{}\n  {}\n\n{}",
        theme.error("rclone is not installed."),
        theme.info("Choose one of these installation options:"),
        theme.muted("If you have Homebrew installed, use this option:"),
        theme.accent("brew install rclone"),
        theme.muted("If you do not have Homebrew, use this option:"),
        theme.accent("sudo -v ; curl https://rclone.org/install.sh | sudo bash"),
        theme.muted(&format!("Then run `{retry_command}` again.")),
    )
}

/// Pull a plain integer count out of the first matching `<label>` line, e.g.
/// `Deleted:                1 (files), 0 (dirs), 10 B (freed)` -> `1` or
/// `Errors:                 2 (fatal error encountered)` -> `2`. The value is
/// always the first whitespace-separated token after the label.
fn simple_count(output: &str, label: &str) -> u64 {
    output
        .lines()
        .find_map(|l| l.trim().strip_prefix(label))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|n| n.trim_end_matches(',').replace(',', "").parse().ok())
        .unwrap_or(0)
}

/// Pull the FILE count out of rclone's `Transferred:` lines. rclone prints
/// two lines with this label: a bytes line (`Transferred:   6 B / 6 B, 100%,
/// 0 B/s, ETA -`) and, only when at least one file moved, a file-count line
/// (`Transferred:            1 / 1, 100%`). We want the latter: the line
/// whose value is immediately followed by a bare `/` token, not a byte unit
/// like `B`. When no file-count line is present (nothing transferred), this
/// yields 0, matching rclone's own behavior of omitting that line entirely.
fn transferred_count(output: &str) -> u64 {
    output
        .lines()
        .filter_map(|l| l.trim().strip_prefix("Transferred:"))
        .find_map(|rest| {
            let mut tokens = rest.split_whitespace();
            let num = tokens.next()?;
            (tokens.next() == Some("/"))
                .then(|| num.parse().ok())
                .flatten()
        })
        .unwrap_or(0)
}

/// Parse rclone's stderr/stdout text + exit success into a `RunOutcome`.
///
/// Defensive: unrecognized counts default to 0, but a non-zero exit with an
/// unrecognized reason is `AbortKind::Other` so verification treats it as
/// needs-attention rather than silently "clean".
#[must_use]
pub fn parse_outcome(exit_ok: bool, output: &str) -> RunOutcome {
    let lc = output.to_ascii_lowercase();
    let abort = if exit_ok {
        None
    } else if lc.contains("--max-delete") || lc.contains("too many deletes") {
        Some(AbortKind::MaxDelete)
    } else if lc.contains("--check-access")
        || lc.contains("access test failed")
        || lc.contains("check file check failed")
    {
        Some(AbortKind::CheckAccess)
    } else if lc.contains("cannot find prior")
        || lc.contains("must run --resync")
        || lc.contains("run --resync")
    {
        Some(AbortKind::PriorListingMissing)
    } else {
        Some(AbortKind::Other)
    };
    RunOutcome {
        exit_ok,
        transferred: transferred_count(output),
        deleted: simple_count(output, "Deleted:"),
        errors: simple_count(output, "Errors:"),
        abort,
    }
}

/// Run `rclone <args>` with `env` injected, streaming its progress live.
///
/// rclone's raw stderr (its progress/log stream) is captured for
/// `parse_outcome` (abort/error detection still needs the raw text), but what
/// the user actually sees is a clean, themed rendering: each apply-phase line
/// is classified with [`progress::classify_applied`] and, if it renders to a
/// display line via [`progress::render_applied`], that themed line is written
/// to our stderr in place of the raw rclone chatter (noise is suppressed
/// entirely). Copied/deleted counts are tallied from these classified events
/// rather than trusted from rclone's summary block, since `--stats-one-line`
/// drops the `Transferred: N/M` line `parse_outcome` used to read. stdout is
/// inherited; only stderr is piped and drained on this thread (no deadlock:
/// the single owned pipe is drained continuously, and stdout is never
/// buffered by us).
#[must_use]
pub fn run_rclone(
    reporter: &super::current::Reporter,
    env: &[(String, String)],
    args: &[String],
) -> RunOutcome {
    crate::logging::log(format!("spawn rclone args={args:?} env_keys={}", env.len()));
    let mut cmd = Command::new("rclone");
    cmd.args(args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::piped());

    let Ok(mut child) = cmd.spawn() else {
        crate::logging::log("spawn rclone failed");
        return RunOutcome {
            exit_ok: false,
            transferred: 0,
            deleted: 0,
            errors: 0,
            abort: Some(AbortKind::Other),
        };
    };

    let theme = crate::theme::Theme::active();
    let mut captured = String::new();
    let mut copied = 0_usize;
    let mut deleted = 0_usize;
    if let Some(stderr) = child.stderr.take() {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            crate::logging::log(format!("rclone stderr {line}"));
            captured.push_str(&line);
            captured.push('\n');

            let clean = progress::strip(&line);
            let event = progress::classify_applied(&clean);
            if let Some(ev) = &event {
                match ev {
                    progress::Applied::Copied(_) => copied += 1,
                    progress::Applied::Deleted(_) => deleted += 1,
                    _ => {}
                }
                if let Some(display) = progress::render_applied(ev, theme) {
                    reporter.line(&display);
                }
            }
        }
    }

    let exit_ok = child.wait().is_ok_and(|status| status.success());
    crate::logging::log(format!("rclone exited success={exit_ok}"));
    let mut outcome = parse_outcome(exit_ok, &captured);
    outcome.transferred = u64::try_from(copied).unwrap_or(0);
    outcome.deleted = u64::try_from(deleted).unwrap_or(0);
    outcome
}

/// Run rclone capturing combined output, without streaming or display.
///
/// For read-only probes like `brain check`'s dry-run. Returns `(exit_ok,
/// combined_output)`. Unlike [`run_rclone`], nothing is printed here: the
/// caller (`check::run`) decides what the user sees.
#[must_use]
pub fn run_rclone_capture(env: &[(String, String)], args: &[String]) -> (bool, String) {
    crate::logging::log(format!(
        "spawn rclone capture args={args:?} env_keys={}",
        env.len()
    ));
    let mut cmd = Command::new("rclone");
    cmd.args(args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    match cmd.output() {
        Ok(o) => {
            let mut text = String::from_utf8_lossy(&o.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&o.stderr));
            crate::logging::log(format!(
                "rclone capture exited success={} bytes={}",
                o.status.success(),
                text.len()
            ));
            (o.status.success(), text)
        }
        Err(err) => {
            crate::logging::log(format!("spawn rclone capture failed: {err}"));
            (false, String::new())
        }
    }
}

#[cfg(test)]
mod tests;
