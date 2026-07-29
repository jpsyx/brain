//! Follow an in-flight sync's live progress from another process.
//!
//! When `brain sync` finds that a sync is already running (a detached
//! background sync, or another shell), it does **not** start a second one and
//! it does **not** error. It attaches: it tails `current.log` — the running
//! sync's progress record (see [`crate::sync::current`]) — mirroring each new
//! line to the terminal until the sync finishes, then prints the final
//! outcome from the journal. Ctrl-C here stops only the follower; the sync
//! keeps running in its own process.

use std::io::Write as _;
use std::path::Path;
use std::time::Duration;

use crate::sync::command;
use crate::sync::current;
use crate::sync::journal::Journal;
use crate::theme::Theme;

/// How often the follower re-reads the log while the sync runs.
const POLL: Duration = Duration::from_millis(300);

/// Split off the portion of `content` not yet consumed, given how many bytes
/// were already emitted. Returns the new slice and the updated offset.
///
/// Handles a truncated log (a fresh run reset `current.log` to empty, so it is
/// now shorter than our offset) by restarting from the beginning.
#[must_use]
pub fn appended(content: &str, already: usize) -> (&str, usize) {
    let start = if already <= content.len() { already } else { 0 };
    (&content[start..], content.len())
}

/// Attach to the running sync and mirror its progress until it ends.
///
/// Then print the final journal outcome. Best-effort throughout: unreadable
/// files or a missing journal degrade to less output, never an error.
pub fn follow_until_done() {
    follow_with(&current::log_path(), Theme::active(), current::is_running, POLL);
}

fn follow_with<F: Fn() -> bool>(log: &Path, theme: Theme, is_running: F, poll: Duration) {
    let mut err = std::io::stderr();
    let _ = writeln!(
        err,
        "{}",
        theme.info("A sync is already running; following its progress (Ctrl-C to stop watching, the sync keeps going)…")
    );
    let mut offset = 0usize;
    loop {
        offset = mirror(log, &mut err, offset);
        if !is_running() {
            offset = mirror(log, &mut err, offset);
            break;
        }
        std::thread::sleep(poll);
    }
    let _ = offset;
    if let Ok(journal) = Journal::open(&Journal::default_path()) {
        if let Ok(recent) = journal.recent(1) {
            let _ = writeln!(err, "{}", command::format_last_run(recent.first(), theme));
        }
    }
}

/// Read the log, emit whatever is new since `offset`, and return the new offset.
fn mirror<W: std::io::Write>(log: &Path, out: &mut W, offset: usize) -> usize {
    let Ok(content) = std::fs::read_to_string(log) else {
        return offset;
    };
    let (new, next) = appended(&content, offset);
    if !new.is_empty() {
        let _ = write!(out, "{new}");
        let _ = out.flush();
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appended_returns_only_the_new_tail() {
        let (new, off) = appended("line one\n", 0);
        assert_eq!(new, "line one\n");
        assert_eq!(off, 9);
        let (new, off) = appended("line one\nline two\n", off);
        assert_eq!(new, "line two\n");
        assert_eq!(off, 18);
    }

    #[test]
    fn appended_yields_nothing_when_no_growth() {
        let (new, off) = appended("same\n", 5);
        assert_eq!(new, "");
        assert_eq!(off, 5);
    }

    #[test]
    fn appended_restarts_when_the_log_was_truncated() {
        // A new run reset current.log; it is now shorter than our old offset.
        let (new, off) = appended("fresh\n", 999);
        assert_eq!(new, "fresh\n");
        assert_eq!(off, 6);
    }

    #[test]
    fn follow_drains_the_log_and_stops_when_the_sync_ends() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let dir = std::env::temp_dir().join(format!("brain-follow-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("current.log");
        std::fs::write(&log, "phase one\nphase two\n").unwrap();

        // "Running" for the first two polls, then done.
        let ticks = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&ticks);
        let out = follow_capture(&log, Theme::dark(false), move || {
            seen.fetch_add(1, Ordering::SeqCst) < 2
        });

        assert!(out.contains("phase one"), "{out}");
        assert!(out.contains("phase two"), "{out}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Test helper: run the follow loop with a zero poll and capture the mirror
    /// output (not the header/journal footer) into a string.
    fn follow_capture<F: Fn() -> bool>(log: &Path, _theme: Theme, is_running: F) -> String {
        let mut buf: Vec<u8> = Vec::new();
        let mut offset = 0usize;
        loop {
            offset = mirror(log, &mut buf, offset);
            if !is_running() {
                offset = mirror(log, &mut buf, offset);
                break;
            }
        }
        let _ = offset;
        String::from_utf8(buf).unwrap()
    }
}
