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
            if s == ".git"
                || s == ".cache"
                || s == ".DS_Store"
                || s == "node_modules"
                || s == "__pycache__"
                || s.ends_with(".pyc")
            {
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
mod tests;
