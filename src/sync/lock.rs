//! A machine-wide advisory lock so only one sync runs at a time.
//!
//! All sync entry points (manual `brain sync`, the start/exit hooks, the
//! watcher) acquire it; whoever can't skips (best-effort, never blocks). The
//! lockfile at `~/.cache/brain/sync/sync.lock` holds the owning PID; a crash
//! leaves a stale file the next acquire reaps via PID-liveness (or a generous
//! age backstop).

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Age backstop behind the PID-liveness reap: a lock older than this is treated
/// as stale even if some unrelated process now holds its PID.
const STALE_AGE: Duration = Duration::from_secs(600);

/// `~/.cache/brain/sync/sync.lock` — beside the journal (machine-local cache).
#[must_use]
pub fn default_path() -> PathBuf {
    let base = std::env::var_os("HOME").map_or_else(
        || PathBuf::from("."),
        |h| PathBuf::from(h).join(".cache").join("brain").join("sync"),
    );
    base.join("sync.lock")
}

/// Pure staleness decision: a lock is stale if its owner is gone or it is older
/// than the age backstop.
#[must_use]
pub fn is_stale(owner_alive: bool, age: Duration, cap: Duration) -> bool {
    !owner_alive || age >= cap
}

/// Held lock; removes the lockfile on drop.
pub struct Guard {
    path: PathBuf,
}

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
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
        Ok(()) => Some(Guard { path: path.to_path_buf() }),
        Err(()) => {
            if lock_is_stale(path) {
                let _ = fs::remove_file(path);
                create_exclusive(path).ok().map(|()| Guard { path: path.to_path_buf() })
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
    let age = fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .unwrap_or(Duration::ZERO);
    is_stale(crate::server::lifecycle::pid_alive(pid), age, STALE_AGE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_stale_true_when_owner_gone_or_too_old() {
        let cap = Duration::from_secs(600);
        assert!(is_stale(false, Duration::ZERO, cap)); // dead owner
        assert!(is_stale(true, Duration::from_secs(700), cap)); // over the cap
        assert!(!is_stale(true, Duration::from_secs(1), cap)); // live + young
    }

    #[test]
    fn default_path_is_under_cache_brain_sync() {
        assert!(default_path().ends_with(".cache/brain/sync/sync.lock"));
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
