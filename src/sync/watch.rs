//! The filesystem watcher.
//!
//! A **pure** debounce state machine + a path-relevance predicate, plus the thin
//! `notify` shell that feeds them. The watcher thread runs in-process for the
//! shell's lifetime, but when it fires it only *spawns a detached background
//! sync* (`--if-idle`); it never runs the sync itself. The sync's own writes
//! under the root re-arm the debouncer, but a spawn that lands while a sync
//! still holds the lock coalesces (exits silently), so there is no loop.

use std::path::{Component, Path};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::sync::config::SyncConfig;

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
            if s.contains(crate::sync::args::CONFLICT_MARKER) {
                return false;
            }
        }
    }
    true
}

/// Stops the watcher thread when dropped.
///
/// Dropping the inner `Watcher` closes the event channel, so the loop observes
/// `Disconnected` and exits. We do **not** join the thread: shell teardown must
/// never block on an in-flight sync (spec §10); a detached final pass is harmless
/// (the lock coalesces it).
pub struct WatcherHandle {
    _watcher: RecommendedWatcher,
}

/// Start watching `root` recursively; call `on_fire` once each time changes
/// settle for `window`.
///
/// The one relevant IO/thread shell over the pure `Debouncer` +
/// `is_watch_relevant`; `on_fire` runs synchronously in the loop, so events
/// during it buffer in the channel and coalesce (spec §6).
pub fn spawn_watcher_with<F>(
    root: &Path,
    window: Duration,
    on_fire: F,
) -> anyhow::Result<WatcherHandle>
where
    F: Fn() + Send + 'static,
{
    let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })?;
    watcher.watch(root, RecursiveMode::Recursive)?;

    std::thread::spawn(move || {
        let mut deb = Debouncer::new(window);
        loop {
            let now = Instant::now();
            // Block indefinitely when disarmed; else only until the fire is due.
            let recv = match deb.time_until_fire(now) {
                None => rx.recv().map_err(|_| ()),
                Some(d) => match rx.recv_timeout(d) {
                    Ok(ev) => Ok(ev),
                    Err(mpsc::RecvTimeoutError::Timeout) => Err(()), // maybe fire
                    Err(mpsc::RecvTimeoutError::Disconnected) => break, // handle dropped
                },
            };
            match recv {
                Ok(Ok(event)) => {
                    if event.paths.iter().any(|p| is_watch_relevant(p)) {
                        deb.on_event(Instant::now());
                    }
                }
                Ok(Err(_)) => {} // a notify error event; ignore
                Err(()) => {
                    // Either the channel closed (recv error) or a timeout elapsed.
                    if deb.poll(Instant::now()) {
                        on_fire();
                    } else {
                        // recv() returned Disconnected (handle dropped) → stop.
                        break;
                    }
                }
            }
        }
    });

    Ok(WatcherHandle { _watcher: watcher })
}

/// Start the real auto-sync watcher: fires a locked bidirectional sync when
/// changes under `root` settle for the configured debounce window.
pub fn spawn_watcher(root: &Path, cfg: &SyncConfig) -> anyhow::Result<WatcherHandle> {
    spawn_watcher_with(root, cfg.debounce(), || {
        crate::sync::trigger::spawn_detached_sync(crate::sync::args::Direction::Both);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::time::{Duration, Instant};

    #[test]
    fn disarmed_debouncer_never_fires() {
        let mut d = Debouncer::new(Duration::from_secs(3));
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
    fn excludes_the_raw_rclone_conflict_marker() {
        // The marker rclone leaves before the friendly rename must not re-trigger
        // a sync (mirrors the bisync `*.__brainconflict__*` exclude).
        assert!(!is_watch_relevant(Path::new("notes/idea.md.__brainconflict__")));
    }

    #[test]
    fn ordinary_notes_and_csvs_are_relevant() {
        assert!(is_watch_relevant(Path::new("projects/x/note.md")));
        assert!(is_watch_relevant(Path::new("tasks/tasks.csv")));
    }
}
