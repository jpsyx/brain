//! Workspace-scoped interprocess ownership for task-store mutations.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use rusqlite::Connection;

use crate::workspace::WorkspaceContext;

pub(crate) struct TaskStoreOwner {
    _connection: Connection,
    lock_path: PathBuf,
}

impl TaskStoreOwner {
    pub(crate) fn acquire(workspace: &WorkspaceContext) -> Result<Self> {
        Self::acquire_path(&workspace.paths().task_store_lock())
    }

    pub(crate) fn acquire_path(lock_path: &Path) -> Result<Self> {
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating task lock directory {}", parent.display()))?;
        }
        let connection = Connection::open(lock_path)
            .with_context(|| format!("opening task lock {}", lock_path.display()))?;
        connection.busy_timeout(Duration::from_secs(30))?;
        connection
            .execute_batch("PRAGMA journal_mode = OFF; BEGIN IMMEDIATE")
            .with_context(|| format!("acquiring task lock {}", lock_path.display()))?;
        Ok(Self {
            _connection: connection,
            lock_path: lock_path.to_path_buf(),
        })
    }

    pub(crate) fn verify(&self, workspace: &WorkspaceContext) -> Result<()> {
        if self.lock_path != workspace.paths().task_store_lock() {
            bail!("task-store owner belongs to a different workspace");
        }
        Ok(())
    }
}
