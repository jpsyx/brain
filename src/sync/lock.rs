//! A workspace-scoped advisory lock so only one sync per workspace runs at a time.
//!
//! All sync entry points (manual `brain sync`, the start/exit hooks, the
//! watcher) acquire it; whoever can't skips (best-effort, never blocks). The
//! UUID-scoped lockfile holds the owning PID; a crash
//! leaves a stale file the next acquire reaps via PID-liveness.

use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Seek as _, Write as _};
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
    owner: String,
    file: File,
    heartbeat: Option<Heartbeat>,
}

impl Drop for Guard {
    fn drop(&mut self) {
        drop(self.heartbeat.take());
        let still_ours = read_owner_path(&self.path).as_deref() == Some(self.owner.as_str());
        if still_ours {
            let _ = fs::remove_file(&self.path);
        }
        let _ = fs2::FileExt::unlock(&self.file);
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

fn guard_for(
    path: &Path,
    owner: String,
    file: File,
    heartbeat_interval: Duration,
) -> std::io::Result<Guard> {
    let heartbeat_file = file.try_clone()?;
    Ok(Guard {
        heartbeat: Some(start_heartbeat(
            path.to_path_buf(),
            owner.clone(),
            heartbeat_file,
            heartbeat_interval,
        )),
        path: path.to_path_buf(),
        owner,
        file,
    })
}

fn start_heartbeat(path: PathBuf, owner: String, mut file: File, interval: Duration) -> Heartbeat {
    let (stop, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        while rx.recv_timeout(interval).is_err() {
            if !refresh_lock_if_owned(&path, &owner, &mut file) {
                break;
            }
        }
    });
    Heartbeat {
        stop,
        handle: Some(handle),
    }
}

fn refresh_lock_if_owned(path: &Path, owner: &str, file: &mut File) -> bool {
    if read_owner_path(path).as_deref() != Some(owner) {
        return false;
    }
    write_owner(file, owner).is_ok()
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
    try_acquire_with_existing_hook(path, heartbeat_interval, || {})
}

fn try_acquire_with_existing_hook(
    path: &Path,
    heartbeat_interval: Duration,
    before_existing_lock: impl FnOnce(),
) -> Option<Guard> {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    match publish_new(path, heartbeat_interval) {
        Publish::Acquired(guard) => Some(guard),
        Publish::Failed => None,
        Publish::Exists => try_takeover_existing(path, heartbeat_interval, before_existing_lock),
    }
}

enum Publish {
    Acquired(Guard),
    Exists,
    Failed,
}

fn publish_new(path: &Path, heartbeat_interval: Duration) -> Publish {
    publish_new_with_writer(path, heartbeat_interval, write_owner)
}

fn publish_new_with_writer(
    path: &Path,
    heartbeat_interval: Duration,
    writer: impl FnOnce(&mut File, &str) -> std::io::Result<()>,
) -> Publish {
    let owner = format!("{} {}", std::process::id(), uuid::Uuid::new_v4());
    let Some(parent) = path.parent() else {
        return Publish::Failed;
    };
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("lock");
    let pending = parent.join(format!(".{file_name}.{}.pending", uuid::Uuid::new_v4()));
    let Ok(mut file) = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&pending)
    else {
        return Publish::Failed;
    };
    if writer(&mut file, &owner).is_err()
        || file.sync_data().is_err()
        || fs2::FileExt::try_lock_exclusive(&file).is_err()
    {
        let _ = fs::remove_file(&pending);
        return Publish::Failed;
    }
    let published = match fs::hard_link(&pending, path) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(_) => {
            let _ = fs::remove_file(&pending);
            return Publish::Failed;
        }
    };
    let _ = fs::remove_file(&pending);
    if !published {
        return Publish::Exists;
    }
    guard_for(path, owner, file, heartbeat_interval).map_or_else(
        |_| {
            let _ = fs::remove_file(path);
            Publish::Failed
        },
        Publish::Acquired,
    )
}

fn try_takeover_existing(
    path: &Path,
    heartbeat_interval: Duration,
    before_existing_lock: impl FnOnce(),
) -> Option<Guard> {
    let mut existing = OpenOptions::new().read(true).write(true).open(path).ok()?;
    before_existing_lock();
    if fs2::FileExt::try_lock_exclusive(&existing).is_err() {
        return None;
    }
    let observed = read_owner_file(&mut existing)?;
    if read_owner_path(path).as_deref() != Some(observed.as_str())
        || !lock_record_is_stale(&observed, &existing)
    {
        return None;
    }
    fs::remove_file(path).ok()?;
    match publish_new(path, heartbeat_interval) {
        Publish::Acquired(guard) => Some(guard),
        Publish::Exists | Publish::Failed => None,
    }
}

fn write_owner(file: &mut File, owner: &str) -> std::io::Result<()> {
    file.rewind()?;
    file.set_len(0)?;
    file.write_all(owner.as_bytes())?;
    file.write_all(b"\n")?;
    file.flush()
}

fn read_owner_file(file: &mut File) -> Option<String> {
    file.rewind().ok()?;
    let mut owner = String::new();
    file.read_to_string(&mut owner).ok()?;
    Some(owner.trim().to_owned())
}

fn read_owner_path(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|owner| owner.trim().to_owned())
}

fn lock_record_is_stale(record: &str, file: &File) -> bool {
    let Some(pid) = record
        .split_whitespace()
        .next()
        .and_then(|pid| pid.parse::<u32>().ok())
    else {
        return true;
    };
    let age = file
        .metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .unwrap_or(Duration::ZERO);
    is_stale(crate::server::lifecycle::pid_alive(pid), age, STALE_AGE)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};

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

    #[test]
    fn concurrent_stale_takeover_has_exactly_one_winner() {
        let dir = std::env::temp_dir().join(format!(
            "brain-lock-stale-race-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sync.lock");
        std::fs::write(&path, b"4294967295").unwrap();
        let barrier = Arc::new(Barrier::new(3));
        let spawn_contender = || {
            let path = path.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                try_acquire_with_existing_hook(&path, HEARTBEAT_INTERVAL, || {
                    barrier.wait();
                })
            })
        };
        let contenders = [spawn_contender(), spawn_contender()];

        barrier.wait();
        let guards = contenders
            .into_iter()
            .filter_map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>();

        assert_eq!(guards.len(), 1, "only one stale-lock contender may win");
        assert!(path.exists());
        drop(guards);
        assert!(!path.exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn owner_write_failure_never_publishes_a_visible_lock() {
        let dir = std::env::temp_dir().join(format!(
            "brain-lock-write-failure-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sync.lock");

        let result = publish_new_with_writer(&path, HEARTBEAT_INTERVAL, |_, _| {
            Err(std::io::Error::other("injected owner write failure"))
        });

        assert!(matches!(result, Publish::Failed));
        assert!(
            !path.exists(),
            "a partial owner record must never be visible"
        );
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        std::fs::remove_dir_all(&dir).ok();
    }
}
