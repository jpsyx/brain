//! Machine-wide paths owned by the shared server process.

use std::path::{Path, PathBuf};

/// All machine-global shared-server artifacts below one cache directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerPaths {
    directory: PathBuf,
}

impl ServerPaths {
    /// Derive the shared-server directory from an explicit home directory.
    #[must_use]
    pub fn from_home(home: &Path) -> Self {
        Self {
            directory: home.join(".cache").join("brain").join("server"),
        }
    }

    /// Build paths around an explicit server directory.
    #[must_use]
    pub fn from_directory(directory: PathBuf) -> Self {
        Self { directory }
    }

    /// The owner directory for all machine-wide server infrastructure.
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Generation-tagged process record.
    #[must_use]
    pub fn process_record(&self) -> PathBuf {
        self.directory.join("process.json")
    }

    /// Machine-wide Unix control socket.
    #[must_use]
    pub fn control_socket(&self) -> PathBuf {
        self.directory.join("control.sock")
    }

    /// Atomic starter-election and process-ownership lock.
    #[must_use]
    pub fn election_lock(&self) -> PathBuf {
        self.directory.join("election.lock")
    }

    /// Shared-process lifecycle log.
    #[must_use]
    pub fn log(&self) -> PathBuf {
        self.directory.join("server.log")
    }
}

impl Default for ServerPaths {
    fn default() -> Self {
        let home = std::env::var_os("HOME").map_or_else(PathBuf::new, PathBuf::from);
        Self::from_home(&home)
    }
}
