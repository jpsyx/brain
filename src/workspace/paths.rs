//! Paths that belong to exactly one workspace's machine-local runtime state.

use std::path::{Path, PathBuf};

use super::WorkspaceId;

/// Workspace-scoped machine-local runtime paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspacePaths {
    cache_dir: PathBuf,
}

impl WorkspacePaths {
    /// Derive workspace-local runtime paths below the supplied home directory.
    #[must_use]
    pub fn new(home: &Path, id: WorkspaceId) -> Self {
        Self {
            cache_dir: home
                .join(".cache")
                .join("brain")
                .join("workspaces")
                .join(id.to_string()),
        }
    }

    /// The workspace's machine-local runtime cache directory.
    #[must_use]
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// The workspace-scoped persistent state database.
    #[must_use]
    pub fn state_db(&self) -> PathBuf {
        self.cache_dir.join("state.db")
    }

    /// The workspace-scoped TUI lock file.
    #[must_use]
    pub fn tui_lock(&self) -> PathBuf {
        self.cache_dir.join("tui.lock")
    }

    /// The workspace-scoped receiver inbox directory.
    #[must_use]
    pub fn inbox_dir(&self) -> PathBuf {
        self.cache_dir.join("inbox")
    }

    /// The workspace-scoped receiver responses directory.
    #[must_use]
    pub fn responses_dir(&self) -> PathBuf {
        self.cache_dir.join("responses")
    }

    /// The workspace-scoped logs directory.
    #[must_use]
    pub fn logs_dir(&self) -> PathBuf {
        self.cache_dir.join("logs")
    }

    /// The workspace-scoped synchronization runtime directory.
    #[must_use]
    pub fn sync_dir(&self) -> PathBuf {
        self.cache_dir.join("sync")
    }
}
