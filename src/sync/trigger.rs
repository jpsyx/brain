//! Shell-facing sync triggers. All reuse `command::sync_once` under the sync
//! lock and are best-effort: a held lock, an unconfigured brain, or a spawn
//! failure is swallowed — a trigger never crashes or blocks the shell.

use std::process::{Command, Stdio};

use crate::sync::args::Direction;
use crate::sync::config::SyncConfig;
use crate::sync::{command, lock};

/// Run one sync now, under the lock, synchronously. No-op (returns immediately)
/// when sync is unconfigured or another sync holds the lock. Used by the watcher
/// (in its own thread) and by `sync_in_background`.
pub fn run_locked_sync(dir: Direction) {
    let cfg = SyncConfig::load();
    if !cfg.is_configured() {
        return;
    }
    let Some(_guard) = lock::try_acquire(&lock::default_path()) else {
        return; // another sync is running; skip (coalesce)
    };
    let Ok(root) = crate::paths::brain_root() else {
        return;
    };
    let now = chrono::Utc::now();
    let ts = now.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let date = now.format("%Y-%m-%d").to_string();
    let _ = command::sync_once(&cfg, &root, dir, (&ts, &ts, &date));
    // _guard drops here, releasing the lock.
}

/// Kick one background sync on a detached thread and return at once — used by the
/// `on_start` hook so shell startup never blocks on the network.
pub fn sync_in_background() {
    std::thread::spawn(|| run_locked_sync(Direction::Both));
}

/// Spawn `brain sync` as a fully detached child (own process group, null stdio)
/// so it outlives the shell — used by the `on_exit` hook. The child acquires the
/// lock itself; if a sync is already running it skips (that run covers the exit).
pub fn spawn_detached_sync() {
    use std::os::unix::process::CommandExt as _;
    if let Ok(exe) = std::env::current_exe() {
        let _ = Command::new(exe)
            .arg("sync")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn();
    }
}
