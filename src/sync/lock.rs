//! A machine-wide advisory lock so only one sync runs at a time.
//!
//! All sync entry points (manual `brain sync`, the start/exit hooks, the
//! watcher) acquire it; whoever can't skips (best-effort, never blocks). The
//! lockfile at `~/.cache/brain/sync/sync.lock` holds the owning PID; a crash
//! leaves a stale file the next acquire reaps via PID-liveness.

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

/// `~/.cache/brain/sync/sync.lock` — beside the journal (machine-local cache).
#[must_use]
pub fn default_path() -> PathBuf {
    let base = std::env::var_os("HOME").map_or_else(
        || PathBuf::from("."),
        |h| PathBuf::from(h).join(".cache").join("brain").join("sync"),
    );
    base.join("sync.lock")
}

/// Pure staleness decision: a lock is stale exactly when its owner process is no
/// longer alive.
///
/// PID-liveness is authoritative for this machine-local lock: a live owner is
/// never reaped, so a long-running sync (a large first sync can take many
/// minutes) holds the lock for as long as it needs without a second sync
/// stealing it. The only residual cost is a rare wedge: if a holder is
/// SIGKILLed *and* its PID is later recycled to an unrelated live process, the
/// stale file at `~/.cache/brain/sync/sync.lock` must be removed by hand.
#[must_use]
pub fn is_stale(owner_alive: bool) -> bool {
    !owner_alive
}

/// Held lock; removes the lockfile on drop.
pub struct Guard {
    path: PathBuf,
    pid: u32,
}

impl Drop for Guard {
    fn drop(&mut self) {
        // Remove the lockfile only if it still holds *our* PID, so a Guard whose
        // lock was reaped out from under it (a crash-recovery race) never deletes
        // the new owner's lock.
        let still_ours = fs::read_to_string(&self.path)
            .ok()
            .and_then(|t| t.trim().parse::<u32>().ok())
            == Some(self.pid);
        if still_ours {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn guard_for(path: &Path) -> Guard {
    Guard { path: path.to_path_buf(), pid: std::process::id() }
}

/// Try to acquire the lock without blocking.
///
/// `Some(Guard)` when we took it (no live lock existed, or a stale one was
/// reaped); `None` when a live sync holds it — the caller should skip. Atomic
/// via `create_new` (O_EXCL).
#[must_use]
pub fn try_acquire(path: &Path) -> Option<Guard> {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    match create_exclusive(path) {
        Ok(()) => Some(guard_for(path)),
        Err(()) => {
            if lock_is_stale(path) {
                let _ = fs::remove_file(path);
                create_exclusive(path).ok().map(|()| guard_for(path))
            } else {
                None
            }
        }
    }
}

/// Atomically create the lockfile with our PID; `Err(())` if it already exists.
fn create_exclusive(path: &Path) -> Result<(), ()> {
    let Ok(mut f) = OpenOptions::new().write(true).create_new(true).open(path) else {
        return Err(());
    };
    let _ = write!(f, "{}", std::process::id());
    Ok(())
}

/// Read the lockfile's PID + mtime age and classify staleness (thin IO around
/// `is_stale`). A missing/garbage lockfile is treated as stale (reapable).
fn lock_is_stale(path: &Path) -> bool {
    let Ok(text) = fs::read_to_string(path) else { return true };
    let Ok(pid) = text.trim().parse::<u32>() else { return true };
    is_stale(crate::server::lifecycle::pid_alive(pid))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_stale_tracks_owner_liveness() {
        assert!(is_stale(false)); // dead owner → reapable
        assert!(!is_stale(true)); // live owner → never reaped, however long it runs
    }

    #[test]
    fn default_path_is_under_cache_brain_sync() {
        assert!(default_path().ends_with(".cache/brain/sync/sync.lock"));
    }

    #[test]
    fn drop_only_removes_the_lock_if_it_still_holds_our_pid() {
        // A Guard whose lock was reaped out from under it (crash-recovery race)
        // must not delete the new owner's lock on drop.
        let dir = std::env::temp_dir().join(format!("brain-lock-mine-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sync.lock");

        let g = try_acquire(&path).expect("acquire");
        // Simulate another process reaping our lock and taking it over (pid 1 =
        // init, always alive on unix, and never our pid).
        std::fs::write(&path, b"1").unwrap();
        drop(g);
        assert!(path.exists(), "drop must not delete a lock now owned by another pid");

        std::fs::remove_file(&path).ok();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn second_acquire_is_blocked_until_the_first_drops() {
        // The current process's PID is alive, so a held lock is not stale.
        let dir = std::env::temp_dir().join(format!("brain-lock-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sync.lock");

        let g1 = try_acquire(&path).expect("first acquire takes the lock");
        assert!(try_acquire(&path).is_none(), "second acquire is blocked by the live lock");
        drop(g1);
        let g3 = try_acquire(&path).expect("acquire succeeds after the first drops");
        drop(g3);

        std::fs::remove_dir_all(&dir).ok();
    }
}
