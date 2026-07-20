//! Emit shell-side directives to stdout for the zsh wrapper.
//!
//! Protocol: each line is `key=value`. The wrapper recognizes:
//!   `cd=<path>`        cd into <path>
//!   `claude=<message>` invoke the `cl` alias with <message>
//!   `open=<path>`      hand <path> to the system `open` command
//!   `edit=<path>`      open <path> in the user's terminal editor
//!                      (`$VISUAL`, then `$EDITOR`, then `vi`)
//! Anything else (clap help, errors, etc.) the wrapper prints verbatim, so
//! `b --help` and friends keep working.
//!
//! For interactive paths (TUI, Finder reveal), nothing is written.
//!
//! Each directive has a `*_to` variant that writes into an arbitrary
//! `io::Write` sink; the public wrappers target stdout. Tests drive the
//! `*_to` variants against a byte buffer so the wire protocol stays a
//! checked contract (the zsh wrapper parses these exact strings).

use std::io::{self, Write};
use std::path::Path;

pub fn cd(path: &Path) {
    cd_to(&mut io::stdout(), path);
}

pub fn claude(home: &Path, message: &str) {
    claude_to(&mut io::stdout(), home, message);
}

pub fn open(path: &Path) {
    open_to(&mut io::stdout(), path);
}

pub fn edit(path: &Path) {
    edit_to(&mut io::stdout(), path);
}

pub fn cd_to<W: Write>(out: &mut W, path: &Path) {
    let _ = writeln!(out, "cd={}", path.display());
}

pub fn claude_to<W: Write>(out: &mut W, home: &Path, message: &str) {
    let _ = writeln!(out, "cd={}", home.display());
    let _ = writeln!(out, "claude={message}");
}

pub fn open_to<W: Write>(out: &mut W, path: &Path) {
    let _ = writeln!(out, "open={}", path.display());
}

pub fn edit_to<W: Write>(out: &mut W, path: &Path) {
    let _ = writeln!(out, "edit={}", path.display());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn rendered<F: FnOnce(&mut Vec<u8>)>(f: F) -> String {
        let mut buf = Vec::new();
        f(&mut buf);
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn cd_emits_single_cd_line() {
        let out = rendered(|b| cd_to(b, &PathBuf::from("/tmp/x")));
        assert_eq!(out, "cd=/tmp/x\n");
    }

    #[test]
    fn claude_emits_cd_then_claude() {
        let out = rendered(|b| claude_to(b, &PathBuf::from("/home/brain"), "hello world"));
        assert_eq!(out, "cd=/home/brain\nclaude=hello world\n");
    }

    #[test]
    fn claude_with_empty_message_still_emits_claude_directive() {
        // The wrapper keys off the *presence* of the claude= line, not its
        // value, so an empty message must still emit the directive.
        let out = rendered(|b| claude_to(b, &PathBuf::from("/home/brain"), ""));
        assert_eq!(out, "cd=/home/brain\nclaude=\n");
    }

    #[test]
    fn open_and_edit_use_their_own_keys() {
        assert_eq!(
            rendered(|b| open_to(b, &PathBuf::from("/a/b.pdf"))),
            "open=/a/b.pdf\n"
        );
        assert_eq!(
            rendered(|b| edit_to(b, &PathBuf::from("/a/b.md"))),
            "edit=/a/b.md\n"
        );
    }
}
