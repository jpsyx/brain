//! Invoke `rclone` (thin `Command` shell) and parse its summary into a typed
//! `RunOutcome`. Only the parser is unit-tested; the process spawn is a thin
//! shell exercised via the integration path.

use std::process::Command;

/// Why a bisync aborted, when it did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbortKind {
    /// `--max-delete` guard tripped (rclone's "too many deletes" safety abort).
    MaxDelete,
    /// Baseline listings missing — needs `brain sync init` / `--resync`.
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
            (tokens.next() == Some("/")).then(|| num.parse().ok()).flatten()
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
    } else if lc.contains("cannot find prior") || lc.contains("must run --resync") || lc.contains("run --resync") {
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

/// Run `rclone <args>` with `env` injected, capturing combined output. Returns
/// the parsed outcome, or an `AbortKind::Other` outcome if rclone can't spawn.
#[must_use]
pub fn run_rclone(env: &[(String, String)], args: &[String]) -> RunOutcome {
    let mut cmd = Command::new("rclone");
    cmd.args(args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    match cmd.output() {
        Ok(out) => {
            let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&out.stderr));
            parse_outcome(out.status.success(), &text)
        }
        Err(_) => RunOutcome { exit_ok: false, transferred: 0, deleted: 0, errors: 0, abort: Some(AbortKind::Other) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real `rclone bisync -v` output for a run that resynced 5 new files:
    /// note the TWO `Transferred:` lines (bytes, then file count) — only the
    /// file-count one (`5 / 5, 100%`) should be picked up.
    #[test]
    fn parses_real_transferred_two_line_format() {
        let out = "Transferred:   \t         50 B / 50 B, 100%, 0 B/s, ETA -\n\
                    Checks:                 0 / 0, -, Listed 10\n\
                    Transferred:            5 / 5, 100%\n\
                    Server Side Copies:     5 @ 50 B\n\
                    Elapsed time:         0.0s\n";
        let o = parse_outcome(true, out);
        assert_eq!(o.transferred, 5);
    }

    /// Real output for a delete-only run: no file-count `Transferred:` line
    /// is printed at all (rclone omits it when nothing transferred), and
    /// `Deleted:` carries a `(files), (dirs), (freed)` breakdown.
    #[test]
    fn parses_real_deleted_only_format() {
        let out = "Transferred:   \t          0 B / 0 B, -, 0 B/s, ETA -\n\
                    Checks:                10 / 10, 100%, Listed 6\n\
                    Deleted:                1 (files), 0 (dirs), 10 B (freed)\n\
                    Elapsed time:         0.0s\n";
        let o = parse_outcome(true, out);
        assert_eq!(o.transferred, 0);
        assert_eq!(o.deleted, 1);
    }

    /// Real output for a run that hit fatal errors building listings.
    #[test]
    fn parses_real_errors_format() {
        let out = "Transferred:   \t          0 B / 0 B, -, 0 B/s, ETA -\n\
                    Errors:                 2 (fatal error encountered)\n\
                    Checks:                 2 / 2, 100%, Listed 0\n\
                    Elapsed time:         0.0s\n";
        let o = parse_outcome(false, out);
        assert_eq!(o.errors, 2);
    }

    #[test]
    fn detects_max_delete_abort() {
        // Real wording: rclone's safety-abort message says "too many
        // deletes", never the literal flag name "--max-delete" or "max
        // delete".
        let o = parse_outcome(
            false,
            "ERROR : Safety abort: too many deletes (>50%, 1 of 1) on Path1 \"/a/\". Run with --force if desired.\n\
             NOTICE: Bisync aborted. Please try again.\n\
             NOTICE: Failed to bisync: too many deletes\n",
        );
        assert_eq!(o.abort, Some(AbortKind::MaxDelete));
    }

    #[test]
    fn detects_prior_listing_missing() {
        // Real wording captured from `rclone bisync` against a path with no
        // prior baseline listings.
        let o = parse_outcome(
            false,
            "ERROR : Bisync critical error: cannot find prior Path1 or Path2 listings, likely due to critical error on prior run\n\
             ERROR : Bisync aborted. Must run --resync to recover.\n\
             NOTICE: Failed to bisync: bisync aborted\n",
        );
        assert_eq!(o.abort, Some(AbortKind::PriorListingMissing));
    }

    #[test]
    fn unknown_nonzero_exit_is_other_not_clean() {
        assert_eq!(parse_outcome(false, "something went wrong").abort, Some(AbortKind::Other));
    }
}
