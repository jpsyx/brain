//! The filesystem watcher: a **pure** debounce state machine + a path-relevance
//! predicate.
//!
//! A later slice adds the thin `notify` shell that feeds them. The watcher runs
//! in-process for the shell's lifetime; when it fires, one locked sync runs
//! synchronously in the watcher thread, so the sync's own writes buffer in the
//! event channel and coalesce into at most one no-op follow-up (no loop).

// The `notify` shell that consumes these lands in the next slice; until then the
// binary crate has no caller for them, so scope a `dead_code` allow here. Remove
// when `spawn_watcher` wires them up.
#![allow(dead_code)]

use std::path::{Component, Path};
use std::time::{Duration, Instant};

/// Coalesces a stream of filesystem events into "fire once things go quiet"
/// decisions. Pure: `now` is injected, so it tests without sleeps or a clock.
pub struct Debouncer {
    window: Duration,
    deadline: Option<Instant>,
}

impl Debouncer {
    #[must_use]
    pub fn new(window: Duration) -> Self {
        Self { window, deadline: None }
    }

    /// A relevant event arrived: (re)arm the quiescence timer.
    pub fn on_event(&mut self, now: Instant) {
        self.deadline = Some(now + self.window);
    }

    /// How long until a fire is due, or `None` when disarmed. `Some(0)` means
    /// "fire now" — the watcher loop uses this as its `recv_timeout`.
    #[must_use]
    pub fn time_until_fire(&self, now: Instant) -> Option<Duration> {
        self.deadline.map(|d| d.saturating_duration_since(now))
    }

    /// Fire iff the quiescence window has elapsed; disarms on fire.
    pub fn poll(&mut self, now: Instant) -> bool {
        match self.deadline {
            Some(d) if now >= d => {
                self.deadline = None;
                true
            }
            _ => false,
        }
    }

    #[must_use]
    pub fn is_armed(&self) -> bool {
        self.deadline.is_some()
    }
}

/// Whether a changed path should trigger a sync. Mirrors the bisync exclude set
/// (spec §6): VCS/cache/OS cruft and existing conflict copies never trigger.
#[must_use]
pub fn is_watch_relevant(path: &Path) -> bool {
    for comp in path.components() {
        if let Component::Normal(os) = comp {
            let s = os.to_string_lossy();
            if s == ".git" || s == ".cache" || s == ".DS_Store" {
                return false;
            }
            if s.contains("(conflict ") && s.contains(')') {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::time::{Duration, Instant};

    #[test]
    fn disarmed_debouncer_never_fires() {
        let mut d = Debouncer::new(Duration::from_secs(3));
        assert!(!d.is_armed());
        assert!(!d.poll(Instant::now()));
        assert_eq!(d.time_until_fire(Instant::now()), None);
    }

    #[test]
    fn fires_once_after_the_window_then_disarms() {
        let t0 = Instant::now();
        let mut d = Debouncer::new(Duration::from_secs(3));
        d.on_event(t0);
        assert!(!d.poll(t0 + Duration::from_secs(1)), "not yet quiet");
        assert!(d.poll(t0 + Duration::from_secs(3)), "fires at the window");
        assert!(!d.poll(t0 + Duration::from_secs(4)), "disarmed after firing");
    }

    #[test]
    fn a_burst_coalesces_into_a_single_fire() {
        let t0 = Instant::now();
        let mut d = Debouncer::new(Duration::from_secs(3));
        d.on_event(t0);
        d.on_event(t0 + Duration::from_secs(1)); // re-arms → deadline = t0+4s
        d.on_event(t0 + Duration::from_secs(2)); // re-arms → deadline = t0+5s
        assert!(!d.poll(t0 + Duration::from_secs(4)), "still within the extended window");
        assert!(d.poll(t0 + Duration::from_secs(5)), "one fire once the burst settles");
        assert!(!d.poll(t0 + Duration::from_secs(6)));
    }

    #[test]
    fn time_until_fire_counts_down() {
        let t0 = Instant::now();
        let mut d = Debouncer::new(Duration::from_secs(3));
        d.on_event(t0);
        assert_eq!(d.time_until_fire(t0), Some(Duration::from_secs(3)));
        assert_eq!(d.time_until_fire(t0 + Duration::from_secs(3)), Some(Duration::ZERO));
    }

    #[test]
    fn excludes_vcs_cache_os_cruft_and_conflict_copies() {
        assert!(!is_watch_relevant(Path::new(".git/index")));
        assert!(!is_watch_relevant(Path::new("notes/.DS_Store")));
        assert!(!is_watch_relevant(Path::new(".cache/x")));
        assert!(!is_watch_relevant(Path::new("notes/idea (conflict mac 2026-07-25).md")));
    }

    #[test]
    fn ordinary_notes_and_csvs_are_relevant() {
        assert!(is_watch_relevant(Path::new("projects/x/note.md")));
        assert!(is_watch_relevant(Path::new("tasks/tasks.csv")));
    }
}
