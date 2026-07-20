//! The `markdown-to-pdf` prerequisite: discovery (PATH → conventional bin
//! dirs → login-shell resolution of a function wrapper), validation, the red
//! fail-fast message, and the process-exiting startup gate.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Result, anyhow};

use super::render::{ERROR, color_enabled, paint};
use super::store::home_dir;
use super::vars::{get, set};

/// True when `p` is a regular file with an executable bit set.
fn is_executable_file(p: &Path) -> bool {
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
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
        .map(|p| std::env::split_paths(&p).map(|d| d.join("markdown-to-pdf")).collect::<Vec<_>>())
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
pub fn markdown_to_pdf_command() -> Result<PathBuf> {
    match get("markdown_to_pdf_path") {
        Some(p) if is_executable_file(Path::new(&p)) => Ok(PathBuf::from(p)),
        _ => Err(anyhow!(
            "markdown-to-pdf is not configured; run `brain config set markdown_to_pdf_path=<path>`"
        )),
    }
}

/// The red, `❌`-led message shown when the prerequisite can't be satisfied.
/// `configured` carries the offending path when one was set but invalid, so
/// the wording distinguishes "not found" from "misconfigured". `color` gates
/// the ANSI so a captured/piped stderr stays clean; pure for testing.
#[must_use]
fn missing_markdown_to_pdf_message(configured: Option<&str>, color: bool) -> String {
    let head = paint(
        ERROR,
        "❌ brain requires `markdown-to-pdf`, which it can't use.",
        color,
    );
    let detail = configured.map_or_else(
        || {
            "`markdown-to-pdf` is a hard prerequisite: brain runs it to turn markdown\n\
             notes into PDFs. brain couldn't auto-discover it on your PATH, in the\n\
             usual install dirs, or via your login shell.\n"
                .to_owned()
        },
        |p| {
            format!("The configured `markdown_to_pdf_path` is missing or not executable:\n\n    {p}\n")
        },
    );
    format!(
        "{head}\n\n\
         {detail}\n\
         Install `markdown-to-pdf`, or point brain at it:\n\n    \
         brain config set markdown_to_pdf_path=/path/to/markdown-to-pdf"
    )
}

/// Startup gate for the `markdown-to-pdf` prerequisite.
///
/// A valid configured path passes; an unset one triggers auto-discovery
/// (persisted on success); anything else prints the red message and exits
/// non-zero. Exits directly (not via `anyhow`) so the message prints verbatim
/// without an `Error:` prefix.
pub fn ensure_markdown_to_pdf() {
    let Some(configured) = get("markdown_to_pdf_path") else {
        if let Some(found) = discover_markdown_to_pdf() {
            // Persist for next time; a save failure is non-fatal since the
            // tool itself is present and usable right now.
            let _ = set("markdown_to_pdf_path", &found.display().to_string());
            return;
        }
        fail_missing(None);
    };
    if is_executable_file(Path::new(&configured)) {
        return;
    }
    fail_missing(Some(&configured));
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
    use crate::settings::render::RESET;

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
        assert!(msg.contains(ERROR));
        assert!(msg.contains(RESET));
        assert!(msg.contains("markdown-to-pdf"));
        assert!(msg.contains("brain config set markdown_to_pdf_path="));
    }

    #[test]
    fn missing_message_distinguishes_a_bad_configured_path() {
        let msg = missing_markdown_to_pdf_message(Some("/bad/run.sh"), false);
        assert!(!msg.contains('\x1b')); // color off
        assert!(msg.contains("/bad/run.sh"));
        assert!(msg.contains("missing or not executable"));
    }
}
