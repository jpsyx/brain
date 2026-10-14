//! Atomic, private records with operation-specific ownership.
use super::{CommitRecord, Operation};
use crate::workspace::WorkspaceContext;
use anyhow::{Result, ensure};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

fn path(workspace: &WorkspaceContext, id: &str, suffix: &str) -> Result<PathBuf> {
    let uuid = uuid::Uuid::parse_str(id)?;
    ensure!(
        uuid.to_string() == id,
        "operation ID must be a canonical UUID"
    );
    let path = workspace
        .paths()
        .task_removals_dir()
        .join(format!("{id}.{suffix}"));
    for ancestor in path
        .ancestors()
        .take_while(|ancestor| ancestor.starts_with(workspace.paths().cache_dir()))
    {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => ensure!(
                !metadata.file_type().is_symlink(),
                "operation path is a symbolic link"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(path)
}

pub(crate) struct OperationOwner {
    _connection: rusqlite::Connection,
    path: PathBuf,
}
impl OperationOwner {
    pub(crate) fn acquire(workspace: &WorkspaceContext, id: &str) -> Result<Self> {
        Self::with_timeout(workspace, id, Duration::from_secs(30))
    }

    /// Explicit abandonment rejects a running executor rather than waiting to
    /// act on whatever state that executor eventually leaves behind.
    pub(crate) fn try_acquire(workspace: &WorkspaceContext, id: &str) -> Result<Option<Self>> {
        match Self::with_timeout(workspace, id, Duration::ZERO) {
            Ok(owner) => Ok(Some(owner)),
            Err(error)
                if error
                    .downcast_ref::<rusqlite::Error>()
                    .and_then(rusqlite::Error::sqlite_error_code)
                    .is_some_and(|code| {
                        matches!(
                            code,
                            rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                        )
                    }) =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    fn with_timeout(workspace: &WorkspaceContext, id: &str, timeout: Duration) -> Result<Self> {
        let path = path(workspace, id, "json")?;
        fs::create_dir_all(path.parent().expect("operation parent"))?;
        let lock = super::storage::path(workspace, id, "lock")?;
        let connection = rusqlite::Connection::open(lock)?;
        connection.busy_timeout(timeout)?;
        connection.execute_batch("PRAGMA journal_mode = OFF; BEGIN IMMEDIATE")?;
        Ok(Self {
            _connection: connection,
            path,
        })
    }

    pub(crate) fn save(&self, workspace: &WorkspaceContext, operation: &Operation) -> Result<()> {
        validate_operation(workspace, &operation.operation_id, operation)?;
        ensure!(
            self.path == path(workspace, &operation.operation_id, "json")?,
            "wrong operation owner"
        );
        atomic(&self.path, &serde_json::to_vec_pretty(operation)?, false)
    }
}

#[allow(dead_code)] // Used by the lifecycle facade wired to commands in Task12.
pub(crate) fn load(workspace: &WorkspaceContext, id: &str) -> Result<Operation> {
    let operation: Operation = serde_json::from_slice(&fs::read(path(workspace, id, "json")?)?)?;
    validate_operation(workspace, id, &operation)?;
    Ok(operation)
}

fn validate_operation(workspace: &WorkspaceContext, id: &str, operation: &Operation) -> Result<()> {
    ensure!(
        operation.schema_version == 1
            && operation.operation_id == id
            && operation.workspace_uuid == workspace.id().to_string(),
        "operation does not belong to selected workspace"
    );
    ensure!(
        operation.fingerprint == operation.plan.fingerprint,
        "operation fingerprint mismatch"
    );
    ensure!(
        operation.external.len() == operation.plan.external.len()
            && operation
                .external
                .iter()
                .zip(&operation.plan.external)
                .all(|(progress, action)| {
                    progress.action_id == action.action_id
                        && progress.adapter == action.adapter
                        && progress.issue == action.issue
                }),
        "external progress does not match consented actions"
    );
    Ok(())
}

pub(crate) fn validate_commit(workspace: &WorkspaceContext, record: &CommitRecord) -> Result<()> {
    let _ = path(workspace, &record.receipt.operation_id, "commit.json")?;
    ensure!(
        record.schema_version == 1
            && record.workspace_uuid == workspace.id().to_string()
            && record.receipt.committed,
        "invalid local commit record"
    );
    ensure!(
        !record.receipt.removed.is_empty() && !record.fingerprint.is_empty(),
        "empty local commit record"
    );
    Ok(())
}

pub(crate) fn load_commit(workspace: &WorkspaceContext, id: &str) -> Result<Option<CommitRecord>> {
    let bytes = match fs::read(path(workspace, id, "commit.json")?) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let record: CommitRecord = serde_json::from_slice(&bytes)?;
    validate_commit(workspace, &record)?;
    ensure!(
        record.receipt.operation_id == id,
        "commit operation ID mismatch"
    );
    Ok(Some(record))
}

/// Called with task ownership, never operation ownership, during recovery.
pub(crate) fn persist_commit(workspace: &WorkspaceContext, record: &CommitRecord) -> Result<()> {
    validate_commit(workspace, record)?;
    let id = &record.receipt.operation_id;
    if let Some(existing) = load_commit(workspace, id)? {
        ensure!(
            existing == *record,
            "immutable receipt conflicts with transaction"
        );
        return fs::File::open(workspace.paths().task_removals_dir())?
            .sync_all()
            .map_err(Into::into);
    }
    atomic(
        &path(workspace, id, "commit.json")?,
        &serde_json::to_vec_pretty(record)?,
        true,
    )
}

fn atomic(path: &Path, bytes: &[u8], immutable: bool) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let parent = path.parent().expect("record parent");
    fs::create_dir_all(parent)?;
    // Persist creation of the directory itself before its contents become evidence.
    if let Some(grandparent) = parent.parent() {
        fs::File::open(grandparent)?.sync_all()?;
    }
    let temporary = parent.join(format!(".{}.pending", uuid::Uuid::new_v4()));
    let mut staged = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    let result = (|| -> Result<()> {
        staged.write_all(bytes)?;
        staged.sync_all()?;
        if immutable {
            fs::hard_link(&temporary, path)?;
        } else {
            fs::rename(&temporary, path)?;
        }
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    // Only this invocation's randomly named staging file can be removed here.
    let _ = fs::remove_file(&temporary);
    result
}
