//! How to act on a picked path.
//!
//! The pure decisions (unit-tested without a filesystem or a real
//! `open`/editor):
//!   - `is_textlike`: is this an editable text file (→ `$EDITOR`) or an
//!     opaque blob (→ system `open`)?
//!   - `finder_target`: a file reveals its *parent* directory in Finder;
//!     a directory reveals itself.
//!   - `edit_shell_command` / `iterm_new_tab_applescript`: build the
//!     command + AppleScript that open a text file in a **new iTerm2 tab**.
//!
//! The thin impure spawners (`open_in_editor_tab`, `open_with_system`) shell
//! out fire-and-forget so the persistent brain TUI is never torn down to
//! open a file — a text file opens in a fresh terminal tab, everything else
//! hands off to the system `open`.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Result, bail};

use crate::settings;

/// Is this an editable text file?
///
/// Conservative allowlist of extensions we consider editable text. Files
/// with no extension (`README`, `LICENSE`, `Makefile`, plain notes) are
/// also treated as text since that's the common case in `~/brain`.
#[must_use]
pub fn is_textlike(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|s| s.to_str()) else {
        return true;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        // Notes / markup
        "md" | "markdown" | "mdx" | "mdc" | "mdown"
        | "txt" | "text"
        | "rst" | "org" | "adoc" | "asciidoc"
        | "tex" | "ltx"
        // Data / config
        | "json" | "jsonc" | "yaml" | "yml" | "toml"
        | "ini" | "cfg" | "conf" | "env"
        | "xml" | "csv" | "tsv" | "log"
        // Code (rare in a brain folder, but still text)
        | "rs" | "py" | "js" | "ts" | "jsx" | "tsx" | "mjs" | "cjs"
        | "sh" | "zsh" | "bash" | "fish"
        | "go" | "rb" | "lua" | "java" | "kt" | "swift"
        | "c" | "cpp" | "cc" | "h" | "hpp"
        | "html" | "htm" | "css" | "scss" | "sass"
    )
}

/// Is this a markdown file (the only input `markdown-to-pdf` accepts)?
///
/// Strictly a `.md` extension, case-insensitive — the "Create PDF" command
/// and its `Ctrl-G` shortcut are gated on this. (`.markdown`/`.mdx` are text
/// for editing purposes in `is_textlike`, but the converter only takes `.md`.)
#[must_use]
pub fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("md"))
}

/// Where the generated PDF lands: colocated with the markdown, same stem,
/// `.pdf` extension (`plan.md` → `plan.pdf`).
#[must_use]
pub fn pdf_output_path(md: &Path) -> PathBuf {
    md.with_extension("pdf")
}

/// Convert a markdown file to a colocated same-name PDF and return its path.
///
/// The `markdown-to-pdf` command is resolved from config (see
/// [`crate::settings`]); its `run.sh`-style interface takes `<file.md> --out
/// <file.pdf>`. The converter's non-interactive mode writes a `-vN` variant
/// rather than overwriting, so to guarantee the exact same-name output we drop
/// any existing PDF at the target path first. Blocking (a deliberate action);
/// the caller opens the result and keeps its shell up.
pub fn create_pdf(command: &crate::workspace::CommandContext, md: &Path) -> Result<PathBuf> {
    let out = pdf_output_path(md);
    if out.exists() {
        std::fs::remove_file(&out)?;
    }
    let status = Command::new(settings::markdown_to_pdf_command(command)?)
        .arg(md)
        .arg("--out")
        .arg(&out)
        .status()?;
    if !status.success() {
        bail!("markdown-to-pdf exited with status {status}");
    }
    Ok(out)
}

/// The directory to reveal in Finder for a given selection. Files resolve
/// to their parent directory (mirrors the previous zsh `_brain_pick`); a
/// path with no parent (e.g. `/`) reveals itself.
#[must_use]
pub fn finder_target(path: &Path, is_file: bool) -> &Path {
    if is_file {
        path.parent().unwrap_or(path)
    } else {
        path
    }
}

/// Single-quote a string for safe inclusion in a `sh`/`zsh` command line.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The shell line a new editor tab runs.
///
/// cd into the file's directory, then open it in the user's terminal editor.
/// `${VISUAL:-${EDITOR:-nvim}}` is left unexpanded so the new tab's
/// interactive shell resolves it.
#[must_use]
pub fn edit_shell_command(dir: &Path, file: &Path) -> String {
    format!(
        "cd {} && ${{VISUAL:-${{EDITOR:-nvim}}}} {}",
        shell_quote(&dir.display().to_string()),
        shell_quote(&file.display().to_string()),
    )
}

/// Build the AppleScript that opens `shell_command` in a new iTerm2 tab.
///
/// Targets the current window's new tab. Pure so the wire format stays a
/// checked contract; the `"` and `\` in the command are escaped for the
/// AppleScript string literal.
#[must_use]
pub fn iterm_new_tab_applescript(shell_command: &str) -> String {
    let escaped = shell_command.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "tell application \"iTerm2\"\n\
         \ttell current window\n\
         \t\tset newTab to (create tab with default profile)\n\
         \t\ttell current session of newTab\n\
         \t\t\twrite text \"{escaped}\"\n\
         \t\tend tell\n\
         \tend tell\n\
         end tell"
    )
}

/// True when the host terminal is iTerm2 (so we can open a real new tab).
fn is_iterm() -> bool {
    std::env::var("TERM_PROGRAM").is_ok_and(|t| t == "iTerm.app")
}

/// Open a text file in a new iTerm2 tab.
///
/// The tab cd's to the file's directory, then runs the editor. Falls back to
/// the system `open` on non-iTerm2 terminals, where we can't reliably spawn a
/// tab. Fire-and-forget: the caller's TUI keeps running.
pub fn open_in_editor_tab(file: &Path) -> Result<()> {
    let dir = file.parent().unwrap_or(file);
    if is_iterm() {
        let script = iterm_new_tab_applescript(&edit_shell_command(dir, file));
        let status = Command::new("osascript").arg("-e").arg(&script).status()?;
        if !status.success() {
            bail!("osascript exited with status {status}");
        }
        Ok(())
    } else {
        open_with_system(file)
    }
}

/// Build the AppleScript that moves `path` to the Trash via Finder.
///
/// Finder's `delete` performs a **user-style** delete — the item lands in the
/// Trash and stays recoverable (`Put Back`), unlike an `rm`. Works for both
/// files and directories. Pure so the wire format stays a checked contract;
/// the path is escaped for the AppleScript string literal (`\` then `"`).
#[must_use]
pub fn trash_applescript(path: &Path) -> String {
    let escaped = path
        .display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    format!("tell application \"Finder\" to delete POSIX file \"{escaped}\"")
}

/// Move `path` (a file or directory) to the Trash via Finder. Blocking (a
/// deliberate, confirmed action); the caller keeps its shell up and refreshes.
pub fn move_to_trash(path: &Path) -> Result<()> {
    let status = Command::new("osascript")
        .arg("-e")
        .arg(trash_applescript(path))
        .status()?;
    if !status.success() {
        bail!("osascript (trash) exited with status {status}");
    }
    Ok(())
}

/// Hand a path to the system `open` (default app for files, Finder for
/// directories / reveals). Fire-and-forget.
pub fn open_with_system(path: &Path) -> Result<()> {
    let status = Command::new("open").arg(path).status()?;
    if !status.success() {
        bail!("open exited with status {status}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn markdown_is_textlike() {
        assert!(is_textlike(Path::new("note.md")));
        assert!(is_textlike(Path::new("note.MARKDOWN")));
    }

    #[test]
    fn extensionless_files_are_textlike() {
        assert!(is_textlike(Path::new("README")));
        assert!(is_textlike(Path::new("Makefile")));
    }

    #[test]
    fn extension_match_is_case_insensitive() {
        assert!(is_textlike(Path::new("DATA.CSV")));
        assert!(is_textlike(Path::new("config.YAML")));
    }

    #[test]
    fn binary_blobs_are_not_textlike() {
        assert!(!is_textlike(Path::new("scan.pdf")));
        assert!(!is_textlike(Path::new("photo.png")));
        assert!(!is_textlike(Path::new("archive.zip")));
        assert!(!is_textlike(Path::new("clip.mov")));
    }

    #[test]
    fn only_dot_md_counts_as_markdown() {
        assert!(is_markdown(Path::new("plan.md")));
        assert!(is_markdown(Path::new("PLAN.MD")));
        // The converter takes only .md — richer markdown flavors and
        // non-markdown text are excluded from the PDF command.
        assert!(!is_markdown(Path::new("note.markdown")));
        assert!(!is_markdown(Path::new("note.mdx")));
        assert!(!is_markdown(Path::new("note.txt")));
        assert!(!is_markdown(Path::new("README")));
    }

    #[test]
    fn pdf_output_is_colocated_with_the_same_stem() {
        assert_eq!(
            pdf_output_path(Path::new("/a/b/plan.md")),
            PathBuf::from("/a/b/plan.pdf")
        );
        // A dotted stem keeps everything but the final extension.
        assert_eq!(
            pdf_output_path(Path::new("/a/b/2024.q3.review.md")),
            PathBuf::from("/a/b/2024.q3.review.pdf")
        );
    }

    #[test]
    fn finder_target_of_file_is_its_parent() {
        let p = PathBuf::from("/a/b/c.md");
        assert_eq!(finder_target(&p, true), Path::new("/a/b"));
    }

    #[test]
    fn finder_target_of_dir_is_itself() {
        let p = PathBuf::from("/a/b/c");
        assert_eq!(finder_target(&p, false), Path::new("/a/b/c"));
    }

    #[test]
    fn edit_command_cds_then_opens_editor() {
        let cmd = edit_shell_command(
            Path::new("/Users/x/brain/projects/foo"),
            Path::new("/Users/x/brain/projects/foo/plan.md"),
        );
        assert_eq!(
            cmd,
            "cd '/Users/x/brain/projects/foo' && \
             ${VISUAL:-${EDITOR:-nvim}} '/Users/x/brain/projects/foo/plan.md'"
        );
    }

    #[test]
    fn edit_command_single_quotes_paths_with_spaces() {
        let cmd = edit_shell_command(Path::new("/a b"), Path::new("/a b/c d.md"));
        assert!(cmd.contains("cd '/a b'"));
        assert!(cmd.ends_with("'/a b/c d.md'"));
    }

    #[test]
    fn applescript_embeds_the_command_and_targets_a_new_tab() {
        let script = iterm_new_tab_applescript("cd '/x' && nvim '/x/n.md'");
        assert!(script.contains("create tab with default profile"));
        assert!(script.contains("write text \"cd '/x' && nvim '/x/n.md'\""));
        assert!(script.starts_with("tell application \"iTerm2\""));
    }

    #[test]
    fn applescript_escapes_double_quotes_and_backslashes() {
        let script = iterm_new_tab_applescript(r#"echo "a\b""#);
        // Backslash doubled, inner quotes escaped, so the AppleScript literal
        // stays well-formed.
        assert!(script.contains(r#"write text "echo \"a\\b\"""#));
    }

    #[test]
    fn trash_applescript_asks_finder_to_delete_a_posix_file() {
        let script = trash_applescript(Path::new("/Users/x/brain/projects/old.md"));
        // Finder's `delete` moves the item to the Trash (a user-style delete,
        // recoverable), rather than an unrecoverable `rm`.
        assert!(script.contains("tell application \"Finder\""));
        assert!(script.contains("delete POSIX file"));
        assert!(script.contains("/Users/x/brain/projects/old.md"));
    }

    #[test]
    fn trash_applescript_escapes_quotes_and_backslashes() {
        let script = trash_applescript(Path::new(r#"/a/"weird"\name"#));
        // The path sits inside an AppleScript string literal, so quotes and
        // backslashes must be escaped to keep it well-formed.
        assert!(script.contains(r#"POSIX file "/a/\"weird\"\\name""#));
    }
}
