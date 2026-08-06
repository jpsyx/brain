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
#[path = "election/race_tests.rs"]
mod race_tests;
