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

#[derive(Clone, Copy)]
pub(crate) enum Acquisition {
    Bounded,
    Queued,
}

fn is_contention(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<rusqlite::Error>()
        .is_some_and(|error| {
            matches!(
                error.sqlite_error_code(),
                Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)
            )
        })
}

impl Acquisition {
    pub(crate) fn acquire(self, workspace: &WorkspaceContext) -> Result<TaskStoreOwner> {
        let owner = self.inspect(workspace)?;
        owner.recover(workspace)?;
        Ok(owner)
    }
    pub(crate) fn inspect(self, workspace: &WorkspaceContext) -> Result<TaskStoreOwner> {
        TaskStoreOwner::acquire_path_with(&workspace.paths().task_store_lock(), self)
    }
}

impl TaskStoreOwner {
    pub(crate) fn acquire(workspace: &WorkspaceContext) -> Result<Self> {
        let owner = Self::acquire_path(&workspace.paths().task_store_lock())?;
        owner.recover(workspace)?;
        Ok(owner)
    }

    /// Own the store without recovery, for a read-only pending-journal check.
    pub(crate) fn acquire_inspection(workspace: &WorkspaceContext) -> Result<Self> {
        Self::acquire_path(&workspace.paths().task_store_lock())
    }

    pub(crate) fn acquire_path(lock_path: &Path) -> Result<Self> {
        Self::acquire_path_with(lock_path, Acquisition::Bounded)
    }

    fn acquire_path_with(lock_path: &Path, acquisition: Acquisition) -> Result<Self> {
        loop {
            match Self::attempt(lock_path, acquisition) {
                Err(error)
                    if matches!(acquisition, Acquisition::Queued) && is_contention(&error) =>
                {
                    std::thread::sleep(Duration::from_millis(25));
                }
                result => return result,
            }
        }
    }

    fn attempt(lock_path: &Path, acquisition: Acquisition) -> Result<Self> {
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating task lock directory {}", parent.display()))?;
        }
        let connection = Connection::open(lock_path)
            .with_context(|| format!("opening task lock {}", lock_path.display()))?;
        connection.busy_timeout(match acquisition {
            Acquisition::Bounded => Duration::from_secs(30),
            Acquisition::Queued => Duration::from_millis(100),
        })?;
        connection
            .execute_batch("PRAGMA journal_mode = OFF; BEGIN IMMEDIATE")
            .with_context(|| format!("acquiring task lock {}", lock_path.display()))?;
        Ok(Self {
            _connection: connection,
            lock_path: lock_path.to_path_buf(),
        })
    }

    pub(crate) fn recover(&self, workspace: &WorkspaceContext) -> Result<()> {
        crate::tasks::store_transaction::recover_pending(workspace, self)
    }

    pub(crate) fn verify(&self, workspace: &WorkspaceContext) -> Result<()> {
        self.verify_path(&workspace.paths().task_store_lock())
    }

    pub(crate) fn verify_path(&self, lock_path: &Path) -> Result<()> {
        if self.lock_path != lock_path {
            bail!("task-store owner belongs to a different workspace");
        }
        Ok(())
    }
}

#[cfg(test)]
mod queued_tests {
    use super::*;
    #[test]
    fn queued_acquisition_retries_only_typed_contention() {
        let busy = anyhow::Error::new(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            None,
        ))
        .context("acquiring");
        assert!(is_contention(&busy));
        assert!(!is_contention(&anyhow::anyhow!("database is locked")));
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("lock.db");
        let owner = TaskStoreOwner::acquire_path(&path).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let acquired = TaskStoreOwner::acquire_path_with(&path, Acquisition::Queued).unwrap();
            tx.send(()).unwrap();
            drop(acquired);
        });
        assert!(rx.recv_timeout(Duration::from_millis(150)).is_err());
        drop(owner);
        rx.recv_timeout(Duration::from_secs(3)).unwrap();
        worker.join().unwrap();
    }
}
