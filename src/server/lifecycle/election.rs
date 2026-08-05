//! Atomic shared-process election plus its pure startup decision.

use std::fs::{self, OpenOptions};
use std::io::Write as _;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::{ProcessRecord, ServerGeneration, ServerPaths, pid_alive};

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
        let record = ElectionRecord {
            pid: std::process::id(),
            generation,
        };
        match create_lock(paths, record) {
            Ok(()) => Ok(Some(Self {
                paths: paths.clone(),
                record,
            })),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if live_lock(paths) {
                    return Ok(None);
                }
                remove_lock_if_unchanged(paths)?;
                match create_lock(paths, record) {
                    Ok(()) => Ok(Some(Self {
                        paths: paths.clone(),
                        record,
                    })),
                    Err(retry) if retry.kind() == std::io::ErrorKind::AlreadyExists => Ok(None),
                    Err(retry) => Err(retry).context("creating server election lock"),
                }
            }
            Err(error) => Err(error).context("creating server election lock"),
        }
    }

    /// Transfer an elected starter's token to the spawned server process.
    ///
    /// # Errors
    ///
    /// Returns an error unless the lock contains `generation`.
    pub(super) fn adopt(paths: &ServerPaths, generation: ServerGeneration) -> Result<Self> {
        validate_election_token(paths, generation)?;
        let record = ElectionRecord {
            pid: std::process::id(),
            generation,
        };
        write_lock(paths, record)?;
        Ok(Self {
            paths: paths.clone(),
            record,
        })
    }

    /// Generation represented by this election owner.
    #[must_use]
    pub const fn generation(&self) -> ServerGeneration {
        self.record.generation
    }
}

impl Drop for ElectionGuard {
    fn drop(&mut self) {
        if read_lock(&self.paths) == Some(self.record) {
            let _ = fs::remove_file(self.paths.election_lock());
        }
    }
}

/// Verify that a hidden server run carries the active election token.
///
/// # Errors
///
/// Returns an error when the lock is absent, malformed, or belongs to another
/// generation.
pub fn validate_election_token(paths: &ServerPaths, generation: ServerGeneration) -> Result<()> {
    let Some(record) = read_lock(paths) else {
        anyhow::bail!("server election token is missing or malformed");
    };
    if record.generation != generation {
        anyhow::bail!("server election token does not match this generation");
    }
    Ok(())
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

fn live_lock(paths: &ServerPaths) -> bool {
    read_lock(paths).is_some_and(|record| pid_alive(record.pid))
}

fn remove_lock_if_unchanged(paths: &ServerPaths) -> Result<()> {
    let before = fs::read(paths.election_lock()).ok();
    if before.is_some() && before == fs::read(paths.election_lock()).ok() {
        match fs::remove_file(paths.election_lock()) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("removing stale server election lock"),
        }
    }
    Ok(())
}
