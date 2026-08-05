//! Generation-tagged state for the machine-wide shared server process.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

use anyhow::{Context, Result};

use super::ServerPaths;

/// Identity of one shared-server process generation and its election token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ServerGeneration(Uuid);

impl ServerGeneration {
    /// Create a fresh process generation.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Parse a process generation from a UUID string.
    ///
    /// # Errors
    ///
    /// Returns [`ServerGenerationError`] when `value` is not a UUID.
    pub fn parse(value: &str) -> Result<Self, ServerGenerationError> {
        Uuid::parse_str(value)
            .map(Self)
            .map_err(|_| ServerGenerationError)
    }
}

impl Default for ServerGeneration {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for ServerGeneration {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::str::FromStr for ServerGeneration {
    type Err = ServerGenerationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl Serialize for ServerGeneration {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for ServerGeneration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

/// An invalid shared-server process generation UUID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerGenerationError;

impl Display for ServerGenerationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("server generation must be a UUID")
    }
}

impl Error for ServerGenerationError {}

/// The non-sensitive record published by one running shared server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessRecord {
    /// Operating-system process identity.
    pub pid: u32,
    /// Bound loopback HTTP port.
    pub port: u16,
    /// Unique process generation used to guard cleanup and control messages.
    pub generation: ServerGeneration,
    /// RFC3339 UTC process start timestamp.
    pub started_at: String,
}

/// Read and validate the current process record.
#[must_use]
pub fn read_record(paths: &ServerPaths) -> Option<ProcessRecord> {
    let text = fs::read_to_string(paths.process_record()).ok()?;
    serde_json::from_str(&text).ok()
}

/// Atomically publish one process record inside the shared-server directory.
///
/// # Errors
///
/// Returns an error when the directory, temporary file, permissions, or final
/// replacement cannot be written.
pub fn write_record(paths: &ServerPaths, record: &ProcessRecord) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    fs::create_dir_all(paths.directory())
        .with_context(|| format!("creating {}", paths.directory().display()))?;
    fs::set_permissions(paths.directory(), fs::Permissions::from_mode(0o700))
        .with_context(|| format!("securing {}", paths.directory().display()))?;
    let temporary = paths
        .directory()
        .join(format!("process.{}.tmp", record.generation));
    let bytes = serde_json::to_vec(record).context("serializing server process record")?;
    fs::write(&temporary, bytes).with_context(|| format!("writing {}", temporary.display()))?;
    fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("securing {}", temporary.display()))?;
    fs::rename(&temporary, paths.process_record())
        .with_context(|| format!("publishing {}", paths.process_record().display()))?;
    Ok(())
}

/// Remove the process record and control socket only for `generation`.
///
/// The caller must hold the election lock across this operation so a new
/// generation cannot publish between the generation check and removal.
pub(super) fn remove_generation(paths: &ServerPaths, generation: ServerGeneration) -> Result<bool> {
    if read_record(paths).as_ref().map(|record| record.generation) != Some(generation) {
        return Ok(false);
    }
    remove_if_present(&paths.control_socket())?;
    remove_if_present(&paths.process_record())?;
    Ok(true)
}

/// Remove a control socket created before this generation published a record.
pub(super) fn remove_unpublished(paths: &ServerPaths) -> Result<bool> {
    if read_record(paths).is_some() {
        return Ok(false);
    }
    remove_if_present(&paths.control_socket())?;
    Ok(true)
}

fn remove_if_present(path: &std::path::Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("removing {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::lifecycle::ServerPaths;

    fn generation(value: &str) -> ServerGeneration {
        ServerGeneration::parse(value).expect("valid generation")
    }

    fn record(generation: ServerGeneration) -> ProcessRecord {
        ProcessRecord {
            pid: 42,
            port: 8787,
            generation,
            started_at: "2026-08-04T12:00:00Z".to_owned(),
        }
    }

    #[test]
    fn process_records_round_trip_at_the_owned_path() {
        let temporary = tempfile::tempdir().expect("temporary server directory");
        let paths = ServerPaths::from_directory(temporary.path().join("server"));
        let expected = record(generation("57b162df-983a-45c3-ac7e-bad94eb27a99"));

        write_record(&paths, &expected).expect("write process record");

        assert_eq!(read_record(&paths), Some(expected));
    }

    #[test]
    fn stale_generation_cleanup_cannot_remove_a_winner_record_or_socket() {
        let temporary = tempfile::tempdir().expect("temporary server directory");
        let paths = ServerPaths::from_directory(temporary.path().join("server"));
        let stale = generation("57b162df-983a-45c3-ac7e-bad94eb27a99");
        let winner = generation("91a0cfc2-7427-49d5-a2f1-258f985cd7e5");
        let expected = record(winner);
        write_record(&paths, &expected).expect("write winner record");
        std::fs::write(paths.control_socket(), "winner").expect("write socket marker");

        assert!(!remove_generation(&paths, stale).expect("guarded cleanup"));

        assert_eq!(read_record(&paths), Some(expected));
        assert!(paths.control_socket().exists());
    }

    #[test]
    fn matching_generation_cleanup_removes_record_and_socket() {
        let temporary = tempfile::tempdir().expect("temporary server directory");
        let paths = ServerPaths::from_directory(temporary.path().join("server"));
        let owner = generation("57b162df-983a-45c3-ac7e-bad94eb27a99");
        write_record(&paths, &record(owner)).expect("write owner record");
        std::fs::write(paths.control_socket(), "owner").expect("write socket marker");

        assert!(remove_generation(&paths, owner).expect("guarded cleanup"));

        assert_eq!(read_record(&paths), None);
        assert!(!paths.control_socket().exists());
    }
}
