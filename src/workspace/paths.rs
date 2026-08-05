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

    /// The workspace-scoped socket on which its live TUI accepts jobs.
    #[must_use]
    pub fn job_socket(&self) -> PathBuf {
        self.cache_dir.join("jobs.sock")
    }

    /// The workspace-scoped portable-user transaction lock.
    #[must_use]
    pub fn user_transaction_lock(&self) -> PathBuf {
        self.cache_dir.join("users.transaction.lock")
    }

    /// The workspace-scoped task-store transaction lock.
    #[must_use]
    pub fn task_store_lock(&self) -> PathBuf {
        self.cache_dir.join("tasks.transaction.lock")
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

    /// Workspace-scoped runtime capability artifacts.
    #[must_use]
    pub fn capabilities_dir(&self) -> PathBuf {
        self.cache_dir.join("capabilities")
    }

    /// Claude's selected-only runtime MCP configuration.
    #[must_use]
    pub fn capability_mcp_config(&self) -> PathBuf {
        self.capabilities_dir().join("claude-mcp.json")
    }

    /// Actor-specific selected skill render, isolated from global registries.
    #[must_use]
    pub fn capability_skills_dir(&self, actor: &crate::users::UserId) -> PathBuf {
        self.capabilities_dir()
            .join("actors")
            .join(actor.to_string())
            .join("skills")
    }

    /// The workspace-scoped synchronization runtime directory.
    #[must_use]
    pub fn sync_dir(&self) -> PathBuf {
        self.cache_dir.join("sync")
    }

    /// The workspace-scoped sync lock.
    #[must_use]
    pub fn sync_lock(&self) -> PathBuf {
        self.sync_dir().join("sync.lock")
    }

    /// The workspace-scoped sync journal database.
    #[must_use]
    pub fn sync_journal(&self) -> PathBuf {
        self.sync_dir().join("journal.db")
    }

    /// The workspace-scoped in-flight sync state.
    #[must_use]
    pub fn sync_current_state(&self) -> PathBuf {
        self.sync_dir().join("current.json")
    }

    /// The workspace-scoped in-flight sync log.
    #[must_use]
    pub fn sync_current_log(&self) -> PathBuf {
        self.sync_dir().join("current.log")
    }

    /// The workspace-scoped semantic CSV baseline directory.
    #[must_use]
    pub fn sync_csv_baselines(&self) -> PathBuf {
        self.sync_dir().join("baselines")
    }
}
