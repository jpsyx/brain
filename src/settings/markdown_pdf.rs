//! The `markdown-to-pdf` prerequisite: discovery (PATH → conventional bin
//! dirs → login-shell resolution of a function wrapper), validation, the red
//! fail-fast message, and the process-exiting startup gate.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Result, anyhow};

use crate::theme::Theme;

use super::render::color_enabled;
use super::store::home_dir;

/// True when `p` is a regular file with an executable bit set.
fn is_executable_file(p: &Path) -> bool {
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// Whether the selected workspace's configured PDF command is currently usable.
#[must_use]
pub(crate) fn configured_markdown_to_pdf_ready(command: &crate::workspace::CommandContext) -> bool {
    crate::env::get(command, "markdown_to_pdf_path")
        .as_deref()
        .is_some_and(|path| is_executable_file(Path::new(path)))
}

/// Conventional install dirs to probe for a `markdown-to-pdf` executable,
/// in order. Pure so the search order is a checked contract.
#[must_use]
fn conventional_candidates(home: &Path) -> Vec<PathBuf> {
    [
        home.join(".local").join("bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/opt/homebrew/bin"),
        home.join("bin"),
    ]
    .into_iter()
    .map(|d| d.join("markdown-to-pdf"))
    .collect()
}

/// Absolute-path tokens embedded in shell output (e.g. a resolved command path
/// or the body of an autoloaded function that wraps a script). Splits on
/// whitespace, strips matching surrounding quotes, and keeps only tokens that
/// look like absolute paths — robust to interleaved terminal control junk.
#[must_use]
fn shell_tokens_to_paths(text: &str) -> Vec<PathBuf> {
    text.split_whitespace()
        .map(|t| t.trim_matches(|c| c == '\'' || c == '"'))
        .filter(|t| t.starts_with('/'))
        .map(PathBuf::from)
        .collect()
}

/// Ask the user's login shell to resolve `markdown-to-pdf`. Catches the common
/// case where it is an autoloaded zsh *function* wrapping a real script: we
/// print both the resolved command path and the function body, then mine any
/// executable path out of the result. Best-effort — a missing shell or error
/// yields no candidates.
fn shell_resolved_candidates() -> Vec<PathBuf> {
    let script = "autoload +X markdown-to-pdf 2>/dev/null; \
                  command -v markdown-to-pdf 2>/dev/null; \
                  print -r -- \"${functions[markdown-to-pdf]-}\"";
    Command::new("zsh")
        .arg("-ic")
        .arg(script)
        .output()
        .ok()
        .map(|o| shell_tokens_to_paths(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

/// Find an invokable `markdown-to-pdf`: PATH, then conventional bin dirs, then
/// the login shell. Returns the first candidate that is an executable file.
#[must_use]
pub fn discover_markdown_to_pdf() -> Option<PathBuf> {
    let on_path = std::env::var_os("PATH")
        .map(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join("markdown-to-pdf"))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    on_path
        .into_iter()
        .chain(conventional_candidates(&home_dir()))
        .chain(shell_resolved_candidates())
        .find(|p| is_executable_file(p))
}

/// The validated `markdown-to-pdf` command.
///
/// Assumes [`ensure_markdown_to_pdf`] already ran at startup, but re-validates
/// so a config edited mid-session still errors cleanly rather than spawning a
/// bogus path.
pub fn markdown_to_pdf_command(command: &crate::workspace::CommandContext) -> Result<PathBuf> {
    match crate::env::get(command, "markdown_to_pdf_path") {
        Some(p) if is_executable_file(Path::new(&p)) => Ok(PathBuf::from(p)),
        _ => Err(anyhow!(
            "markdown-to-pdf is not configured; run `brain env set markdown_to_pdf_path=<path>`"
        )),
    }
}

/// The red, `❌`-led message shown when the prerequisite can't be satisfied.
/// `configured` carries the offending path when one was set but invalid, so
/// the wording distinguishes "not found" from "misconfigured". `color` gates
/// the ANSI so a captured/piped stderr stays clean; pure for testing.
#[must_use]
fn missing_markdown_to_pdf_message(configured: Option<&str>, color: bool) -> String {
    let head = Theme::dark(color).error("❌ brain requires `markdown-to-pdf`, which it can't use.");
    let detail = configured.map_or_else(
        || {
            "`markdown-to-pdf` is a hard prerequisite: brain runs it to turn markdown\n\
             notes into PDFs. brain couldn't auto-discover it on your PATH, in the\n\
             usual install dirs, or via your login shell.\n"
                .to_owned()
        },
        |p| {
            format!(
                "The configured `markdown_to_pdf_path` is missing or not executable:\n\n    {p}\n"
            )
        },
    );
    format!(
        "{head}\n\n\
         {detail}\n\
         Install `markdown-to-pdf`, or point brain at it:\n\n    \
         brain env set markdown_to_pdf_path=/path/to/markdown-to-pdf"
    )
}

/// What the startup gate should do, given the configured path's validity and
/// whether discovery turned something up. Pure so the decision is unit-testable
/// without the process-exiting shell.
#[derive(Debug, PartialEq, Eq)]
enum GateAction {
    /// The configured path is valid; nothing to do.
    Pass,
    /// Persist a freshly discovered path, then pass.
    Persist(PathBuf),
    /// Fail with the red message; carries the offending configured path (if any).
    Fail(Option<String>),
}

/// The gate decision. A valid configured path passes. Otherwise (unset, or a
/// stored path that is invalid on *this* machine — e.g. a config.json synced
/// from another host) we prefer a freshly discovered path before failing, so a
/// synced-but-stale `markdown_to_pdf_path` self-heals rather than blocking start.
fn gate_action(
    configured: Option<&str>,
    configured_valid: bool,
    discovered: Option<PathBuf>,
) -> GateAction {
    if configured_valid {
        return GateAction::Pass;
    }
    discovered.map_or_else(
        || GateAction::Fail(configured.map(str::to_owned)),
        GateAction::Persist,
    )
}

/// Startup gate for the `markdown-to-pdf` prerequisite.
///
/// Delegates the decision to [`gate_action`], then applies it: persist a
/// discovered path (non-fatal on save error, since the tool is usable now) or
/// print the red message and exit non-zero. Exits directly (not via `anyhow`)
/// so the message prints verbatim without an `Error:` prefix.
pub fn ensure_markdown_to_pdf(command: &crate::workspace::CommandContext) {
    let configured = crate::env::get(command, "markdown_to_pdf_path");
    let valid = configured
        .as_deref()
        .is_some_and(|p| is_executable_file(Path::new(p)));
    let discovered = if valid {
        None
    } else {
        discover_markdown_to_pdf()
    };
    match gate_action(configured.as_deref(), valid, discovered) {
        GateAction::Pass => {}
        GateAction::Persist(found) => {
            let _ = crate::env::set(
                command,
                "markdown_to_pdf_path",
                &found.display().to_string(),
            );
        }
        GateAction::Fail(configured) => fail_missing(configured.as_deref()),
    }
}

fn fail_missing(configured: Option<&str>) -> ! {
    eprintln!(
        "{}",
        missing_markdown_to_pdf_message(configured, color_enabled())
    );
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conventional_candidates_are_ordered_bins_under_home() {
        let c = conventional_candidates(Path::new("/Users/x"));
        assert_eq!(c[0], PathBuf::from("/Users/x/.local/bin/markdown-to-pdf"));
        assert!(c.iter().all(|p| p.ends_with("markdown-to-pdf")));
    }

    #[test]
    fn shell_tokens_extracts_quoted_absolute_path_from_a_function_body() {
        // The exact shape of an autoloaded launcher function.
        let body = "markdown-to-pdf() {\n\temulate -L zsh\n\t\
                    '/Users/x/src/tool/markdown-to-pdf/run.sh' \"$@\"\n}";
        let paths = shell_tokens_to_paths(body);
        assert!(paths.contains(&PathBuf::from("/Users/x/src/tool/markdown-to-pdf/run.sh")));
        // No bare-word / relative tokens leak through.
        assert!(paths.iter().all(|p| p.is_absolute()));
    }

    #[test]
    fn shell_tokens_ignores_terminal_control_noise() {
        let noisy = "\x1b]1337;RemoteHost=me@host\x07 markdown-to-pdf /opt/x/run.sh";
        assert_eq!(
            shell_tokens_to_paths(noisy),
            vec![PathBuf::from("/opt/x/run.sh")]
        );
    }

    #[test]
    fn missing_message_names_the_tool_and_the_fix_in_red() {
        let msg = missing_markdown_to_pdf_message(None, true);
        assert!(msg.contains('❌'));
        assert!(msg.contains("\x1b[91m")); // error red
        assert!(msg.contains("\x1b[0m")); // reset
        assert!(msg.contains("markdown-to-pdf"));
        assert!(msg.contains("brain env set markdown_to_pdf_path="));
    }

    #[test]
    fn missing_message_distinguishes_a_bad_configured_path() {
        let msg = missing_markdown_to_pdf_message(Some("/bad/run.sh"), false);
        assert!(!msg.contains('\x1b')); // color off
        assert!(msg.contains("/bad/run.sh"));
        assert!(msg.contains("missing or not executable"));
    }

    #[test]
    fn gate_passes_when_the_configured_path_is_valid() {
        assert_eq!(gate_action(Some("/ok/mtp"), true, None), GateAction::Pass);
    }

    #[test]
    fn gate_rediscovers_when_the_stored_path_is_invalid_on_this_machine() {
        // A config synced from another host: the stored path doesn't exist here,
        // but discovery finds a local one → persist it instead of failing.
        let found = PathBuf::from("/opt/homebrew/bin/markdown-to-pdf");
        assert_eq!(
            gate_action(Some("/other/machine/mtp"), false, Some(found.clone())),
            GateAction::Persist(found)
        );
    }

    #[test]
    fn gate_persists_a_discovered_path_when_unset() {
        let found = PathBuf::from("/usr/local/bin/markdown-to-pdf");
        assert_eq!(
            gate_action(None, false, Some(found.clone())),
            GateAction::Persist(found)
        );
    }

    #[test]
    fn gate_fails_when_invalid_and_nothing_discovered() {
        assert_eq!(
            gate_action(Some("/bad/mtp"), false, None),
            GateAction::Fail(Some("/bad/mtp".to_owned()))
        );
        assert_eq!(gate_action(None, false, None), GateAction::Fail(None));
    }
}
