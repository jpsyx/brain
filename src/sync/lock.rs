//! A workspace-scoped advisory lock so only one sync per workspace runs at a time.
//!
//! All sync entry points (manual `brain sync`, the start/exit hooks, the
//! watcher) acquire it; whoever can't skips (best-effort, never blocks). The
//! UUID-scoped lockfile holds the owning PID; a crash
//! leaves a stale file the next acquire reaps via PID-liveness.

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

/// Age backstop behind heartbeat refreshes. A live PID with an old lockfile is
/// treated as stale, closing the SIGKILL + PID-recycle wedge.
const STALE_AGE: Duration = Duration::from_secs(600);

/// How often a live holder refreshes the lockfile mtime.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(60);

/// Pure staleness decision: a lock is stale when its owner process is gone or
/// when its heartbeat has not refreshed within the age cap.
#[must_use]
pub fn is_stale(owner_alive: bool, age: Duration, cap: Duration) -> bool {
    !owner_alive || age >= cap
}

/// Held lock; removes the lockfile on drop.
pub struct Guard {
    path: PathBuf,
    pid: u32,
    heartbeat: Option<Heartbeat>,
}

impl Drop for Guard {
    fn drop(&mut self) {
        drop(self.heartbeat.take());
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

struct Heartbeat {
    stop: Sender<()>,
    handle: Option<JoinHandle<()>>,
}

impl Drop for Heartbeat {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn guard_for(path: &Path, heartbeat_interval: Duration) -> Guard {
    let path = path.to_path_buf();
    let pid = std::process::id();
    Guard {
        heartbeat: Some(start_heartbeat(path.clone(), pid, heartbeat_interval)),
        path,
        pid,
    }
}

fn start_heartbeat(path: PathBuf, pid: u32, interval: Duration) -> Heartbeat {
    let (stop, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        while rx.recv_timeout(interval).is_err() {
            if !refresh_lock_if_owned(&path, pid) {
                break;
            }
        }
    });
    Heartbeat {
        stop,
        handle: Some(handle),
    }
}

fn refresh_lock_if_owned(path: &Path, pid: u32) -> bool {
    let owns_lock = fs::read_to_string(path)
        .ok()
        .and_then(|text| text.trim().parse::<u32>().ok())
        == Some(pid);
    if !owns_lock {
        return false;
    }
    fs::write(path, pid.to_string()).is_ok()
}

/// Try to acquire the lock without blocking.
///
/// `Some(Guard)` when we took it (no live lock existed, or a stale one was
/// reaped); `None` when a live sync holds it — the caller should skip. Atomic
/// via `create_new` (O_EXCL).
#[must_use]
pub fn try_acquire(path: &Path) -> Option<Guard> {
    try_acquire_with_heartbeat(path, HEARTBEAT_INTERVAL)
}

#[must_use]
fn try_acquire_with_heartbeat(path: &Path, heartbeat_interval: Duration) -> Option<Guard> {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    match create_exclusive(path) {
        Ok(()) => Some(guard_for(path, heartbeat_interval)),
        Err(()) => {
            if lock_is_stale(path) {
                let _ = fs::remove_file(path);
                create_exclusive(path)
                    .ok()
                    .map(|()| guard_for(path, heartbeat_interval))
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
    let Ok(text) = fs::read_to_string(path) else {
        return true;
    };
    let Ok(pid) = text.trim().parse::<u32>() else {
        return true;
    };
    let age = fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .unwrap_or(Duration::ZERO);
    is_stale(crate::server::lifecycle::pid_alive(pid), age, STALE_AGE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_stale_tracks_owner_liveness() {
        let cap = std::time::Duration::from_secs(600);

        assert!(is_stale(false, std::time::Duration::ZERO, cap)); // dead owner => reapable
        assert!(is_stale(true, std::time::Duration::from_secs(700), cap));
        assert!(!is_stale(true, std::time::Duration::from_secs(1), cap));
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
        assert!(
            path.exists(),
            "drop must not delete a lock now owned by another pid"
        );

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
        assert!(
            try_acquire(&path).is_none(),
            "second acquire is blocked by the live lock"
        );
        drop(g1);
        let g3 = try_acquire(&path).expect("acquire succeeds after the first drops");
        drop(g3);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn heartbeat_refreshes_the_lockfile_while_held() {
        let dir = std::env::temp_dir().join(format!("brain-lock-heartbeat-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sync.lock");

        let g = try_acquire_with_heartbeat(&path, Duration::from_millis(10)).expect("acquire");
        let before = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .expect("mtime");
        std::thread::sleep(Duration::from_millis(35));
        let after = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .expect("mtime");

        assert!(
            after > before,
            "heartbeat should refresh the lockfile mtime"
        );
        drop(g);
        std::fs::remove_dir_all(&dir).ok();
    }
}
