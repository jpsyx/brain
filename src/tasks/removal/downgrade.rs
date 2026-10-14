//! Downgrade owns operations before the task store and never invokes adapters.
use super::operations::{self, OperationOwner, Phase};
use crate::{tasks::store_lock::TaskStoreOwner, workspace::WorkspaceContext};
use anyhow::{Result, ensure};
use std::{collections::BTreeSet, fs};

pub(crate) fn prepare_downgrade(workspace: &WorkspaceContext) -> Result<DowngradeOwnership> {
    let ids = operation_ids(workspace)?;
    let mut operations = Vec::new();
    for id in &ids {
        operations.push(
            OperationOwner::try_acquire(workspace, id)?
                .ok_or_else(|| anyhow::anyhow!("active removal operation blocks downgrade"))?,
        );
    }
    match fs::symlink_metadata(workspace.paths().task_store_lock()) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
        Ok(metadata) => ensure!(metadata.is_file(), "unsafe task recovery lock"),
    }
    let tasks = TaskStoreOwner::acquire_inspection(workspace)?;
    // A new executor may have appeared while we waited for task ownership.
    // Reject rather than acquiring an operation lock in the opposite order.
    ensure!(
        operation_ids(workspace)? == ids,
        "removal operations changed during downgrade preparation"
    );
    tasks.recover(workspace)?;
    for id in ids {
        let path = workspace
            .paths()
            .task_removals_dir()
            .join(format!("{id}.json"));
        if !path.exists() {
            ensure!(
                operations::load_commit(workspace, &id)?.is_none(),
                "removal commit has no operation record"
            );
            continue; // An idle lock can remain after a rejected plan.
        }
        let operation = operations::load(workspace, &id)?;
        if let Some(committed) = operations::load_commit(workspace, &id)? {
            ensure!(
                committed.fingerprint == operation.fingerprint,
                "removal commit fingerprint mismatch"
            );
        } else {
            ensure!(
                operation.phase == Phase::Abandoned,
                "unresolved removal operation blocks downgrade"
            );
        }
    }
    Ok(DowngradeOwnership {
        _operations: operations,
        _tasks: tasks,
    })
}
pub(crate) struct DowngradeOwnership {
    _operations: Vec<OperationOwner>,
    _tasks: TaskStoreOwner,
}
fn operation_ids(workspace: &WorkspaceContext) -> Result<BTreeSet<String>> {
    let directory = workspace.paths().task_removals_dir();
    match fs::symlink_metadata(&directory) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(error) => return Err(error.into()),
        Ok(metadata) => ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "invalid removal operation directory"
        ),
    }
    let mut ids = BTreeSet::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        ensure!(
            entry.file_type()?.is_file(),
            "invalid removal operation entry"
        );
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| anyhow::anyhow!("invalid removal operation name"))?;
        let id = name
            .strip_suffix(".commit.json")
            .or_else(|| name.strip_suffix(".json"))
            .or_else(|| name.strip_suffix(".lock"))
            .ok_or_else(|| anyhow::anyhow!("unrecognized removal operation artifact"))?;
        ensure!(
            uuid::Uuid::parse_str(id)?.to_string() == id,
            "invalid removal operation ID"
        );
        ids.insert(id.to_owned());
    }
    Ok(ids)
}
