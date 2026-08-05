//! Atomic shared-process election plus its pure startup decision.

use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::{ProcessRecord, ServerGeneration, ServerPaths, pid_alive};

const HANDOFF_CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);
const HANDOFF_CLEANUP_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// The next action for one shared-server startup contender.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartDecision {
    /// Reuse the generation already answering on the control socket.
    Reuse(ProcessRecord),
    /// This contender owns the election and may spawn a new generation.
    Start {
        /// Remove the stale record and socket before binding the new process.
        remove_stale_state: bool,
    },
    /// Another contender owns the election; poll for its published record.
    WaitForWinner,
}

/// Decide whether one contender reuses, starts, or waits for a shared process.
#[must_use]
pub fn decide_start(
    record: Option<&ProcessRecord>,
    pid_alive: bool,
    socket_reachable: bool,
    election_lock: bool,
) -> StartDecision {
    if let Some(record) = record
        && pid_alive
        && socket_reachable
    {
        return StartDecision::Reuse(record.clone());
    }
    if election_lock {
        StartDecision::Start {
            remove_stale_state: record.is_some(),
        }
    } else {
        StartDecision::WaitForWinner
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ElectionRecord {
    pid: u32,
    generation: ServerGeneration,
}

/// Exclusive election ownership for one process generation.
#[derive(Debug)]
pub struct ElectionGuard {
    paths: ServerPaths,
    record: ElectionRecord,
    _mutex: ElectionMutex,
    remove_on_drop: bool,
}

impl ElectionGuard {
    /// Atomically elect this process for `generation`.
    ///
    /// A live owner yields `Ok(None)`. A malformed lock or a lock owned by a
    /// dead process is reaped before one bounded retry.
    ///
    /// # Errors
    ///
    /// Returns an error when the server directory or lock cannot be accessed.
    pub fn try_acquire(paths: &ServerPaths, generation: ServerGeneration) -> Result<Option<Self>> {
        fs::create_dir_all(paths.directory())
            .with_context(|| format!("creating {}", paths.directory().display()))?;
        let Some(mutex) = ElectionMutex::try_acquire(paths)? else {
            return Ok(None);
        };
        let record = ElectionRecord {
            pid: std::process::id(),
            generation,
        };
        if let Some(observed) = read_lock(paths) {
            if pid_alive(observed.pid) {
                return Ok(None);
            }
            if !remove_lock_if_observed(paths, observed)? {
                return Ok(None);
            }
        } else if paths.election_lock().exists() {
            fs::remove_file(paths.election_lock())
                .context("removing malformed server election lock")?;
        }
        create_lock(paths, record).context("creating server election lock")?;
        Ok(Some(Self {
            paths: paths.clone(),
            record,
            _mutex: mutex,
            remove_on_drop: true,
        }))
    }

    /// Transfer an elected starter's token to the spawned server process.
    ///
    /// # Errors
    ///
    /// Returns an error unless the lock contains `generation`.
    pub(super) fn adopt(paths: &ServerPaths, generation: ServerGeneration) -> Result<Self> {
        Self::adopt_for_pid(paths, generation, std::process::id())
    }

    fn adopt_for_pid(paths: &ServerPaths, generation: ServerGeneration, pid: u32) -> Result<Self> {
        let mutex = ElectionMutex::acquire(paths)?;
        let observed = election_record_for_generation(paths, generation)?;
        let record = ElectionRecord { pid, generation };
        if !transfer_lock_if_observed(paths, observed, record)? {
            anyhow::bail!("server election token changed before adoption");
        }
        Ok(Self {
            paths: paths.clone(),
            record,
            _mutex: mutex,
            remove_on_drop: true,
        })
    }

    /// Generation represented by this election owner.
    #[must_use]
    pub const fn generation(&self) -> ServerGeneration {
        self.record.generation
    }

    /// Release the starter mutex while retaining exact cleanup until adoption.
    #[must_use]
    pub fn handoff(mut self) -> ElectionHandoff {
        let handoff = ElectionHandoff {
            paths: self.paths.clone(),
            record: self.record,
        };
        self.remove_on_drop = false;
        handoff
    }
}

impl Drop for ElectionGuard {
    fn drop(&mut self) {
        if self.remove_on_drop && read_lock(&self.paths) == Some(self.record) {
            let _ = fs::remove_file(self.paths.election_lock());
        }
    }
}

/// Parent-side cleanup retained while a spawned child adopts the election.
#[derive(Debug)]
pub struct ElectionHandoff {
    paths: ServerPaths,
    record: ElectionRecord,
}

impl ElectionHandoff {
    /// Finish the handoff by removing an unadopted parent token.
    ///
    /// Adoption or replacement makes cleanup a no-op. Transient mutex
    /// contention is retried within a bounded cleanup window.
    ///
    /// # Errors
    ///
    /// Returns an error when the token cannot be inspected, when the mutex
    /// cannot be acquired within the cleanup window, or when the exact parent
    /// token cannot be removed.
    pub fn cleanup(&self) -> Result<()> {
        let deadline = Instant::now() + HANDOFF_CLEANUP_TIMEOUT;
        loop {
            if inspect_lock(&self.paths)? != Some(self.record) {
                return Ok(());
            }
            match ElectionMutex::try_acquire(&self.paths)? {
                Some(_mutex) => {
                    remove_lock_if_observed(&self.paths, self.record)?;
                    return Ok(());
                }
                None if Instant::now() >= deadline => {
                    anyhow::bail!(
                        "server election handoff cleanup timed out after {HANDOFF_CLEANUP_TIMEOUT:?}"
                    );
                }
                None => std::thread::sleep(HANDOFF_CLEANUP_POLL_INTERVAL),
            }
        }
    }
}

#[derive(Debug)]
struct ElectionMutex {
    file: File,
}

impl ElectionMutex {
    fn acquire(paths: &ServerPaths) -> Result<Self> {
        let file = File::open(paths.directory())
            .with_context(|| format!("opening {}", paths.directory().display()))?;
        fs2::FileExt::lock_exclusive(&file).context("locking server election mutation")?;
        Ok(Self { file })
    }

    fn try_acquire(paths: &ServerPaths) -> Result<Option<Self>> {
        let file = File::open(paths.directory())
            .with_context(|| format!("opening {}", paths.directory().display()))?;
        match fs2::FileExt::try_lock_exclusive(&file) {
            Ok(()) => Ok(Some(Self { file })),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(error) => Err(error).context("locking server election mutation"),
        }
    }
}

impl Drop for ElectionMutex {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}

/// Verify that a hidden server run carries the active election token.
///
/// # Errors
///
/// Returns an error when the lock is absent, malformed, or belongs to another
/// generation.
pub fn validate_election_token(paths: &ServerPaths, generation: ServerGeneration) -> Result<()> {
    election_record_for_generation(paths, generation)?;
    Ok(())
}

fn election_record_for_generation(
    paths: &ServerPaths,
    generation: ServerGeneration,
) -> Result<ElectionRecord> {
    let Some(record) = read_lock(paths) else {
        anyhow::bail!("server election token is missing or malformed");
    };
    if record.generation != generation {
        anyhow::bail!("server election token does not match this generation");
    }
    Ok(record)
}

fn create_lock(paths: &ServerPaths, record: ElectionRecord) -> std::io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(paths.election_lock())?;
    let bytes = serde_json::to_vec(&record).map_err(std::io::Error::other)?;
    file.write_all(&bytes)
}

fn write_lock(paths: &ServerPaths, record: ElectionRecord) -> Result<()> {
    let bytes = serde_json::to_vec(&record).context("serializing server election lock")?;
    fs::write(paths.election_lock(), bytes)
        .with_context(|| format!("writing {}", paths.election_lock().display()))
}

fn read_lock(paths: &ServerPaths) -> Option<ElectionRecord> {
    let bytes = fs::read(paths.election_lock()).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn inspect_lock(paths: &ServerPaths) -> Result<Option<ElectionRecord>> {
    let bytes = match fs::read(paths.election_lock()) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("reading {}", paths.election_lock().display()));
        }
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .with_context(|| format!("parsing {}", paths.election_lock().display()))
}

fn remove_lock_if_observed(paths: &ServerPaths, observed: ElectionRecord) -> Result<bool> {
    if inspect_lock(paths)? != Some(observed) {
        return Ok(false);
    }
    match fs::remove_file(paths.election_lock()) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).context("removing stale server election lock"),
    }
}

fn transfer_lock_if_observed(
    paths: &ServerPaths,
    observed: ElectionRecord,
    replacement: ElectionRecord,
) -> Result<bool> {
    if read_lock(paths) != Some(observed) {
        return Ok(false);
    }
    write_lock(paths, replacement)?;
    Ok(true)
}

#[cfg(test)]
mod race_tests {
    use std::sync::mpsc;
    use std::sync::{Arc, Barrier};
    use std::time::Duration;

    use super::*;

    #[test]
    fn replacement_between_observation_and_reap_or_adoption_is_preserved() {
        let temporary = tempfile::tempdir().expect("temporary server directory");
        let paths = ServerPaths::from_directory(temporary.path().join("server"));
        fs::create_dir_all(paths.directory()).expect("create server directory");
        let stale = record(999_999, "57b162df-983a-45c3-ac7e-bad94eb27a99");
        let replacement = record(std::process::id(), "91a0cfc2-7427-49d5-a2f1-258f985cd7e5");

        create_lock(&paths, stale).expect("create stale owner");
        let observed = read_lock(&paths).expect("observe stale owner");
        replace_at_barrier(&paths, replacement);

        assert!(!remove_lock_if_observed(&paths, observed).expect("conditional reap"));
        assert!(
            !transfer_lock_if_observed(
                &paths,
                observed,
                record(std::process::id(), "00000000-0000-0000-0000-000000000001",)
            )
            .expect("conditional adoption")
        );
        assert_eq!(read_lock(&paths), Some(replacement));
    }

    #[test]
    fn exact_cleanup_recheck_propagates_token_read_error() {
        let temporary = tempfile::tempdir().expect("temporary server directory");
        let paths = ServerPaths::from_directory(temporary.path().join("server"));
        fs::create_dir_all(paths.directory()).expect("create server directory");
        fs::create_dir(paths.election_lock()).expect("create unreadable token directory");
        let observed = record(std::process::id(), "57b162df-983a-45c3-ac7e-bad94eb27a99");

        let cleanup = remove_lock_if_observed(&paths, observed);

        assert!(cleanup.is_err());
    }

    #[test]
    fn exact_cleanup_recheck_propagates_malformed_token_error() {
        let temporary = tempfile::tempdir().expect("temporary server directory");
        let paths = ServerPaths::from_directory(temporary.path().join("server"));
        fs::create_dir_all(paths.directory()).expect("create server directory");
        fs::write(paths.election_lock(), b"not-json").expect("write malformed token");
        let observed = record(std::process::id(), "57b162df-983a-45c3-ac7e-bad94eb27a99");

        let cleanup = remove_lock_if_observed(&paths, observed);

        assert!(cleanup.is_err());
    }

    #[test]
    fn stale_reap_excludes_a_contender_until_the_observed_owner_is_removed() {
        let temporary = tempfile::tempdir().expect("temporary server directory");
        let paths = ServerPaths::from_directory(temporary.path().join("server"));
        fs::create_dir_all(paths.directory()).expect("create server directory");
        let stale = record(999_999, "57b162df-983a-45c3-ac7e-bad94eb27a99");
        let contender = ServerGeneration::parse("91a0cfc2-7427-49d5-a2f1-258f985cd7e5")
            .expect("valid generation");
        create_lock(&paths, stale).expect("create stale owner");
        let reap_started = Arc::new(Barrier::new(2));
        let finish_reap = Arc::new(Barrier::new(2));
        let thread_paths = paths.clone();
        let thread_started = Arc::clone(&reap_started);
        let thread_finish = Arc::clone(&finish_reap);
        let reaper = std::thread::spawn(move || {
            let _mutex = ElectionMutex::acquire(&thread_paths).expect("lock election mutation");
            let observed = read_lock(&thread_paths).expect("observe stale owner");
            thread_started.wait();
            thread_finish.wait();
            assert!(remove_lock_if_observed(&thread_paths, observed).expect("conditional reap"));
        });
        reap_started.wait();

        assert!(
            ElectionGuard::try_acquire(&paths, contender)
                .expect("contending election")
                .is_none()
        );

        finish_reap.wait();
        reaper.join().expect("reaper thread");
        assert!(
            ElectionGuard::try_acquire(&paths, contender)
                .expect("election after reap")
                .is_some()
        );
    }

    #[test]
    fn child_adoption_excludes_contenders_until_transfer_completes() {
        let temporary = tempfile::tempdir().expect("temporary server directory");
        let paths = ServerPaths::from_directory(temporary.path().join("server"));
        let generation = ServerGeneration::parse("57b162df-983a-45c3-ac7e-bad94eb27a99")
            .expect("valid generation");
        let contender = ServerGeneration::parse("91a0cfc2-7427-49d5-a2f1-258f985cd7e5")
            .expect("valid generation");
        let parent = ElectionGuard::try_acquire(&paths, generation)
            .expect("parent election")
            .expect("parent owns election");
        let handoff = parent.handoff();
        let adoption_complete = Arc::new(Barrier::new(2));
        let release_child = Arc::new(Barrier::new(2));
        let thread_paths = paths.clone();
        let thread_adopted = Arc::clone(&adoption_complete);
        let thread_release = Arc::clone(&release_child);
        let child = std::thread::spawn(move || {
            let guard = ElectionGuard::adopt_for_pid(&thread_paths, generation, 999_999)
                .expect("child adoption");
            thread_adopted.wait();
            thread_release.wait();
            drop(guard);
        });
        adoption_complete.wait();
        handoff.cleanup().expect("finish parent handoff");
        assert!(validate_election_token(&paths, generation).is_ok());

        assert!(
            ElectionGuard::try_acquire(&paths, contender)
                .expect("contending election")
                .is_none()
        );

        release_child.wait();
        child.join().expect("child adoption thread");
        assert!(
            ElectionGuard::try_acquire(&paths, contender)
                .expect("election after adoption")
                .is_some()
        );
    }

    #[test]
    fn parent_handoff_cleans_its_exact_token_when_child_is_lost_before_adoption() {
        let temporary = tempfile::tempdir().expect("temporary server directory");
        let paths = ServerPaths::from_directory(temporary.path().join("server"));
        let generation = ServerGeneration::parse("57b162df-983a-45c3-ac7e-bad94eb27a99")
            .expect("valid generation");
        let parent = ElectionGuard::try_acquire(&paths, generation)
            .expect("parent election")
            .expect("parent owns election");
        let child_ready = Arc::new(Barrier::new(2));
        let lose_child = Arc::new(Barrier::new(2));
        let thread_ready = Arc::clone(&child_ready);
        let thread_loss = Arc::clone(&lose_child);
        let child = std::thread::spawn(move || {
            thread_ready.wait();
            thread_loss.wait();
        });
        let handoff = parent.handoff();
        child_ready.wait();

        lose_child.wait();
        child.join().expect("pre-adoption child");
        handoff.cleanup().expect("clean parent handoff");

        assert!(!paths.election_lock().exists());
    }

    #[test]
    fn parent_handoff_cleanup_survives_mutex_contention_after_child_loss() {
        let temporary = tempfile::tempdir().expect("temporary server directory");
        let paths = ServerPaths::from_directory(temporary.path().join("server"));
        let generation = ServerGeneration::parse("57b162df-983a-45c3-ac7e-bad94eb27a99")
            .expect("valid generation");
        let parent = ElectionGuard::try_acquire(&paths, generation)
            .expect("parent election")
            .expect("parent owns election");
        let handoff = parent.handoff();
        let mutex_held = Arc::new(Barrier::new(2));
        let release_mutex = Arc::new(Barrier::new(2));
        let holder_paths = paths.clone();
        let holder_ready = Arc::clone(&mutex_held);
        let holder_release = Arc::clone(&release_mutex);
        let holder = std::thread::spawn(move || {
            let _mutex = ElectionMutex::acquire(&holder_paths).expect("occupy election mutex");
            holder_ready.wait();
            holder_release.wait();
        });
        mutex_held.wait();
        let cleanup_started = Arc::new(Barrier::new(2));
        let cleanup_ready = Arc::clone(&cleanup_started);
        let (cleanup_finished_tx, cleanup_finished_rx) = mpsc::channel();
        let cleanup = std::thread::spawn(move || {
            cleanup_ready.wait();
            handoff.cleanup().expect("clean parent handoff");
            cleanup_finished_tx
                .send(())
                .expect("report handoff cleanup completion");
        });
        cleanup_started.wait();

        let finished_while_contended = cleanup_finished_rx
            .recv_timeout(Duration::from_millis(250))
            .is_ok();
        release_mutex.wait();
        holder.join().expect("mutex holder thread");
        if !finished_while_contended {
            cleanup_finished_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("handoff cleanup after contention");
        }
        cleanup.join().expect("handoff cleanup thread");

        assert!(!paths.election_lock().exists());
    }

    #[test]
    fn parent_handoff_cleanup_propagates_token_read_error() {
        let temporary = tempfile::tempdir().expect("temporary server directory");
        let paths = ServerPaths::from_directory(temporary.path().join("server"));
        let generation = ServerGeneration::parse("57b162df-983a-45c3-ac7e-bad94eb27a99")
            .expect("valid generation");
        let parent = ElectionGuard::try_acquire(&paths, generation)
            .expect("parent election")
            .expect("parent owns election");
        let handoff = parent.handoff();
        let parent_record = handoff.record;
        fs::remove_file(paths.election_lock()).expect("remove parent token");
        fs::create_dir(paths.election_lock()).expect("replace token with unreadable directory");

        let cleanup = handoff.cleanup();

        assert!(cleanup.is_err());
        assert!(paths.election_lock().is_dir());
        fs::remove_dir(paths.election_lock()).expect("remove unreadable directory");
        create_lock(&paths, parent_record).expect("restore parent token");
        handoff.cleanup().expect("retry parent cleanup");
        assert!(!paths.election_lock().exists());
    }

    #[test]
    fn parent_handoff_cleanup_propagates_malformed_token_error() {
        let temporary = tempfile::tempdir().expect("temporary server directory");
        let paths = ServerPaths::from_directory(temporary.path().join("server"));
        let generation = ServerGeneration::parse("57b162df-983a-45c3-ac7e-bad94eb27a99")
            .expect("valid generation");
        let parent = ElectionGuard::try_acquire(&paths, generation)
            .expect("parent election")
            .expect("parent owns election");
        let handoff = parent.handoff();
        let parent_record = handoff.record;
        fs::write(paths.election_lock(), b"not-json").expect("write malformed token");

        let cleanup = handoff.cleanup();

        assert!(cleanup.is_err());
        assert_eq!(
            fs::read(paths.election_lock()).expect("read malformed token"),
            b"not-json"
        );
        write_lock(&paths, parent_record).expect("restore parent token");
        handoff.cleanup().expect("retry parent cleanup");
        assert!(!paths.election_lock().exists());
    }

    fn replace_at_barrier(paths: &ServerPaths, replacement: ElectionRecord) {
        let before_replace = Arc::new(Barrier::new(2));
        let after_replace = Arc::new(Barrier::new(2));
        let thread_paths = paths.clone();
        let thread_before = Arc::clone(&before_replace);
        let thread_after = Arc::clone(&after_replace);
        let replacement_thread = std::thread::spawn(move || {
            thread_before.wait();
            fs::remove_file(thread_paths.election_lock()).expect("remove observed owner");
            create_lock(&thread_paths, replacement).expect("create live replacement");
            thread_after.wait();
        });
        before_replace.wait();
        after_replace.wait();
        replacement_thread.join().expect("replacement thread");
    }

    fn record(pid: u32, generation: &str) -> ElectionRecord {
        ElectionRecord {
            pid,
            generation: ServerGeneration::parse(generation).expect("valid generation"),
        }
    }
}
