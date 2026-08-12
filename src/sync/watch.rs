//! The filesystem watcher.
//!
//! A **pure** debounce state machine + a path-relevance predicate, plus the thin
//! `notify` shell that feeds them. The watcher thread runs in-process for the
//! shell's lifetime, but when it fires it only *spawns a detached background
//! push* (`--if-idle`); it never runs the sync itself. Push mode never writes
//! beneath the watched root, so a completed upload cannot retrigger itself.

use std::path::{Component, Path};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher};

#[cfg(target_os = "macos")]
type BrainWatcher = notify::PollWatcher;
#[cfg(not(target_os = "macos"))]
type BrainWatcher = notify::RecommendedWatcher;

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
        Self {
            window,
            deadline: None,
        }
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
/// (spec §6): VCS/cache/OS cruft, existing conflict copies, and transaction
/// journals/scratch/locks never trigger.
#[must_use]
pub fn is_watch_relevant(path: &Path) -> bool {
    for comp in path.components() {
        if let Component::Normal(os) = comp {
            let s = os.to_string_lossy();
            if s == ".git" || s == ".cache" || s == ".DS_Store" || s == "node_modules" {
                return false;
            }
            if matches!(
                s.as_ref(),
                "package.json" | "package-lock.json" | "bun.lock"
            ) {
                return false;
            }
            if s.contains("(conflict ") && s.contains(')') {
                return false;
            }
            // Transaction scratch: the `.brain-*` journals and their staged /
            // backup / restore files, the `.<live-name>.brain-triage-…` siblings
            // written beside a live file, and any in-root transaction lock.
            if s.starts_with(".brain-")
                || s.contains(".brain-triage-")
                || s.ends_with(".transaction.lock")
            {
                return false;
            }
            if s.contains(crate::sync::args::CONFLICT_MARKER) {
                return false;
            }
        }
    }
    true
}

enum WatchInput {
    Paths(Vec<std::path::PathBuf>),
    Stop,
    #[cfg(test)]
    Poll,
    #[cfg(test)]
    Observed(mpsc::Sender<()>),
}

/// Stops this watcher thread, and no peer workspace's watcher, when dropped.
pub struct WatcherHandle {
    _watcher: BrainWatcher,
    stop: mpsc::Sender<WatchInput>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Drop for WatcherHandle {
    fn drop(&mut self) {
        let _ = self.stop.send(WatchInput::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run_watcher_loop<F, N>(rx: &mpsc::Receiver<WatchInput>, window: Duration, on_fire: F, now: N)
where
    F: Fn() + Send + 'static,
    N: Fn() -> Instant + Send + 'static,
{
    let mut debouncer = Debouncer::new(window);
    loop {
        let received = debouncer.time_until_fire(now()).map_or_else(
            || rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected),
            |wait| rx.recv_timeout(wait),
        );
        match received {
            Ok(WatchInput::Paths(paths)) => {
                if paths.iter().any(|path| is_watch_relevant(path)) {
                    debouncer.on_event(now());
                }
            }
            Ok(WatchInput::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if debouncer.poll(now()) {
                    on_fire();
                }
            }
            #[cfg(test)]
            Ok(WatchInput::Poll) => {
                if debouncer.poll(now()) {
                    on_fire();
                }
            }
            #[cfg(test)]
            Ok(WatchInput::Observed(acknowledge)) => {
                let _ = acknowledge.send(());
            }
        }
    }
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
    let (tx, rx) = mpsc::channel::<WatchInput>();
    let event_tx = tx.clone();
    let handler = move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res {
            let _ = event_tx.send(WatchInput::Paths(event.paths));
        }
    };
    // FSEvents can silently omit changes in otherwise valid user-owned trees
    // on some macOS versions. PollWatcher gives the receiver machine a
    // deterministic fallback without adding a Watchman service dependency.
    #[cfg(target_os = "macos")]
    let mut watcher = notify::PollWatcher::new(
        handler,
        notify::Config::default().with_poll_interval(Duration::from_secs(1)),
    )?;
    #[cfg(not(target_os = "macos"))]
    let mut watcher = notify::RecommendedWatcher::new(handler, notify::Config::default())?;
    watcher.watch(root, RecursiveMode::Recursive)?;

    let worker = std::thread::spawn(move || run_watcher_loop(&rx, window, on_fire, Instant::now));

    Ok(WatcherHandle {
        _watcher: watcher,
        stop: tx,
        worker: Some(worker),
    })
}

/// Start the real auto-sync watcher: fires a one-way push when changes under
/// `root` settle for the configured debounce window.
pub fn spawn_watcher(
    workspace: std::sync::Arc<crate::workspace::WorkspaceContext>,
    cfg: &SyncConfig,
) -> anyhow::Result<WatcherHandle> {
    let root = workspace.root().to_path_buf();
    spawn_watcher_with(&root, cfg.debounce(), move || {
        let _ = crate::sync::trigger::spawn_detached_sync(
            &workspace,
            crate::sync::args::Direction::Push,
        );
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::{Arc, Mutex, mpsc};
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
        assert!(
            !d.poll(t0 + Duration::from_secs(4)),
            "disarmed after firing"
        );
    }

    #[test]
    fn a_burst_coalesces_into_a_single_fire() {
        let t0 = Instant::now();
        let mut d = Debouncer::new(Duration::from_secs(3));
        d.on_event(t0);
        d.on_event(t0 + Duration::from_secs(1)); // re-arms → deadline = t0+4s
        d.on_event(t0 + Duration::from_secs(2)); // re-arms → deadline = t0+5s
        assert!(
            !d.poll(t0 + Duration::from_secs(4)),
            "still within the extended window"
        );
        assert!(
            d.poll(t0 + Duration::from_secs(5)),
            "one fire once the burst settles"
        );
        assert!(!d.poll(t0 + Duration::from_secs(6)));
    }

    #[test]
    fn time_until_fire_counts_down() {
        let t0 = Instant::now();
        let mut d = Debouncer::new(Duration::from_secs(3));
        d.on_event(t0);
        assert_eq!(d.time_until_fire(t0), Some(Duration::from_secs(3)));
        assert_eq!(
            d.time_until_fire(t0 + Duration::from_secs(3)),
            Some(Duration::ZERO)
        );
    }

    #[test]
    fn excludes_vcs_cache_os_cruft_and_conflict_copies() {
        assert!(!is_watch_relevant(Path::new(".git/index")));
        assert!(!is_watch_relevant(Path::new("notes/.DS_Store")));
        assert!(!is_watch_relevant(Path::new(".cache/x")));
        assert!(!is_watch_relevant(Path::new(
            "notes/idea (conflict mac 2026-07-25).md"
        )));
    }

    #[test]
    fn excludes_the_raw_rclone_conflict_marker() {
        // The marker rclone leaves before the friendly rename must not re-trigger
        // a sync (mirrors the bisync `*.__brainconflict__*` exclude).
        assert!(!is_watch_relevant(Path::new(
            "notes/idea.md.__brainconflict__"
        )));
    }

    /// A dependency install is thousands of writes the sync now excludes, so
    /// triggering on them means a debounced sync that transfers nothing, once
    /// per agent launch. It must mirror the exclude set, not lag behind it.
    #[test]
    fn a_dependency_tree_never_triggers_a_sync() {
        assert!(!is_watch_relevant(Path::new(
            ".opencode/node_modules/zod/index.js"
        )));
        assert!(!is_watch_relevant(Path::new(
            "projects/thing/node_modules/x/y.js"
        )));
        assert!(!is_watch_relevant(Path::new(".opencode/package-lock.json")));
        assert!(!is_watch_relevant(Path::new(".opencode/bun.lock")));
        // Brain's own bridge is content, so it still triggers.
        assert!(is_watch_relevant(Path::new(".opencode/plugins/brain.js")));
    }

    /// Mid-transaction is the worst possible moment to trigger a push: the
    /// journal and its scratch are excluded from transfer, so a sync fired by
    /// them can only transfer a half-applied group of live files.
    #[test]
    fn a_transaction_journal_or_lock_never_triggers_a_sync() {
        for path in [
            ".config/.brain-user-transaction.json",
            ".config/.brain-user-4213-17e9-0.staged",
            ".config/.brain-user-4213-17e9-0.backup",
            ".config/.brain-triage-habits-transaction.json",
            "tasks/.tasks.csv.brain-triage-9f2-0.staged",
            "tasks/.brain-task-schema-tasks.staged",
            ".config/.receiver-setup.transaction.lock",
        ] {
            assert!(!is_watch_relevant(Path::new(path)), "{path}");
        }
    }

    #[test]
    fn ordinary_notes_and_csvs_are_relevant() {
        assert!(is_watch_relevant(Path::new("projects/x/note.md")));
        assert!(is_watch_relevant(Path::new("tasks/tasks.csv")));
    }

    #[test]
    fn stopping_one_clock_driven_watcher_loop_does_not_stop_its_peer() {
        let now = Arc::new(Mutex::new(Instant::now()));
        let (personal_tx, personal_rx) = mpsc::channel();
        let (family_tx, family_rx) = mpsc::channel();
        let (fired_tx, fired_rx) = mpsc::channel();
        let personal_clock = Arc::clone(&now);
        let personal_fired = fired_tx.clone();
        let personal = std::thread::spawn(move || {
            run_watcher_loop(
                &personal_rx,
                Duration::from_secs(3),
                move || personal_fired.send("personal").unwrap(),
                move || *personal_clock.lock().unwrap(),
            );
        });
        let family_clock = Arc::clone(&now);
        let family = std::thread::spawn(move || {
            run_watcher_loop(
                &family_rx,
                Duration::from_secs(3),
                move || fired_tx.send("family").unwrap(),
                move || *family_clock.lock().unwrap(),
            );
        });

        personal_tx
            .send(WatchInput::Paths(vec![
                Path::new("personal/note.md").to_path_buf(),
            ]))
            .unwrap();
        family_tx
            .send(WatchInput::Paths(vec![
                Path::new("family/note.md").to_path_buf(),
            ]))
            .unwrap();
        for sender in [&personal_tx, &family_tx] {
            let (observed_tx, observed_rx) = mpsc::channel();
            sender.send(WatchInput::Observed(observed_tx)).unwrap();
            observed_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        }
        *now.lock().unwrap() += Duration::from_secs(3);
        personal_tx.send(WatchInput::Poll).unwrap();
        family_tx.send(WatchInput::Poll).unwrap();
        let first = fired_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let second = fired_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_ne!(first, second);

        personal_tx.send(WatchInput::Stop).unwrap();
        personal.join().unwrap();
        family_tx
            .send(WatchInput::Paths(vec![
                Path::new("family/second.md").to_path_buf(),
            ]))
            .unwrap();
        let (observed_tx, observed_rx) = mpsc::channel();
        family_tx.send(WatchInput::Observed(observed_tx)).unwrap();
        observed_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        *now.lock().unwrap() += Duration::from_secs(3);
        family_tx.send(WatchInput::Poll).unwrap();

        assert_eq!(
            fired_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            "family"
        );
        family_tx.send(WatchInput::Stop).unwrap();
        family.join().unwrap();
    }
}
