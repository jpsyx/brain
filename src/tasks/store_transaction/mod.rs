//! Authenticated, crash-safe grouped replacement with caller-specific target policies.

mod artifacts;
mod journal;
pub(crate) use journal::committed_removal_pending;

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use self::artifacts::{cleanup_artifacts, describe, sync_parent, write_new};
use self::journal::{journal_path, publish_journal, recover_pending_locked};
use crate::tasks::store_lock::TaskStoreOwner;
use crate::workspace::WorkspaceContext;

static NONCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub(crate) struct FileChange {
    pub(crate) path: PathBuf,
    pub(crate) before: Option<Vec<u8>>,
    pub(crate) after: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum JournalState {
    Preparing,
    Prepared,
    Committed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransactionStep {
    Stage(usize),
    Backup(usize),
    Install(usize),
    SyncInstall(usize),
    Restore(usize),
    PublishJournalWrite(JournalState),
    PublishJournalRename(JournalState),
    PublishJournalSync(JournalState),
    CleanupStaged(usize),
    CleanupBackup(usize),
    PublishReceipt,
    RemoveJournal,
    SyncJournalRemoval,
}

#[derive(Debug, Clone)]
struct Prepared {
    pub(super) live: PathBuf,
    pub(super) staged: PathBuf,
    pub(super) backup: PathBuf,
    pub(super) existed: bool,
    before: Option<Vec<u8>>,
    after: Vec<u8>,
}

pub(crate) fn replace_group(
    workspace: &WorkspaceContext,
    owner: &TaskStoreOwner,
    policy: TargetPolicy,
    changes: &[FileChange],
) -> Result<()> {
    replace_group_with_hook(workspace, owner, policy, changes, |_| Ok(()))
}

pub(crate) fn replace_group_with_hook(
    workspace: &WorkspaceContext,
    owner: &TaskStoreOwner,
    policy: TargetPolicy,
    changes: &[FileChange],
    hook: impl FnMut(TransactionStep) -> std::io::Result<()>,
) -> Result<()> {
    replace_group_recorded(workspace, owner, policy, changes, None, hook)
}

pub(crate) fn replace_group_recorded(
    workspace: &WorkspaceContext,
    owner: &TaskStoreOwner,
    policy: TargetPolicy,
    changes: &[FileChange],
    commit: Option<&crate::tasks::removal::operations::CommitRecord>,
    mut hook: impl FnMut(TransactionStep) -> std::io::Result<()>,
) -> Result<()> {
    owner.verify(workspace)?;
    if let Some(record) = commit {
        anyhow::ensure!(
            policy == TargetPolicy::TaskRemoval && !changes.is_empty(),
            "commit requires removal changes"
        );
        crate::tasks::removal::operations::validate_commit(workspace, record)?;
    }
    recover_all_locked(workspace, &mut hook)?;
    if changes.is_empty() {
        return Ok(());
    }
    let prepared = describe(workspace.root(), policy, changes)?;
    publish_journal(
        workspace,
        policy,
        &prepared,
        JournalState::Preparing,
        commit,
        &mut hook,
    )?;
    let result = transact(workspace, policy, &prepared, commit, &mut hook);
    if let Err(error) = result {
        return match recover_all_locked(workspace, &mut hook) {
            Ok(()) => Err(error),
            Err(recovery) => Err(anyhow!("{error:#}; recovery also failed: {recovery:#}")),
        };
    }
    Ok(())
}

fn transact(
    workspace: &WorkspaceContext,
    policy: TargetPolicy,
    prepared: &[Prepared],
    commit: Option<&crate::tasks::removal::operations::CommitRecord>,
    hook: &mut impl FnMut(TransactionStep) -> std::io::Result<()>,
) -> Result<()> {
    for (index, change) in prepared.iter().enumerate() {
        if let Some(parent) = change.live.parent() {
            fs::create_dir_all(parent)?;
        }
        hook(TransactionStep::Stage(index))?;
        write_new(&change.staged, &change.after)?;
        if let Some(before) = &change.before {
            hook(TransactionStep::Backup(index))?;
            write_new(&change.backup, before)?;
        }
        sync_parent(&change.live)?;
    }
    publish_journal(
        workspace,
        policy,
        prepared,
        JournalState::Prepared,
        commit,
        hook,
    )?;
    for (index, change) in prepared.iter().enumerate() {
        hook(TransactionStep::Install(index))?;
        fs::rename(&change.staged, &change.live)
            .with_context(|| format!("atomically replacing {}", change.live.display()))?;
        hook(TransactionStep::SyncInstall(index))?;
        sync_parent(&change.live)?;
    }
    publish_journal(
        workspace,
        policy,
        prepared,
        JournalState::Committed,
        commit,
        hook,
    )?;
    journal::publish_receipt(workspace, commit, hook)?;
    cleanup_artifacts(prepared, hook)?;
    journal::remove_journal(&journal_path(workspace, policy), hook)
}

pub(crate) fn recover_pending(workspace: &WorkspaceContext, owner: &TaskStoreOwner) -> Result<()> {
    owner.verify(workspace)?;
    recover_all_locked(workspace, &mut |_| Ok(()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TargetPolicy {
    ManagedTriage,
    TaskRemoval,
}

fn recover_all_locked(
    workspace: &WorkspaceContext,
    hook: &mut impl FnMut(TransactionStep) -> std::io::Result<()>,
) -> Result<()> {
    recover_pending_locked(workspace, TargetPolicy::ManagedTriage, hook)?;
    recover_pending_locked(workspace, TargetPolicy::TaskRemoval, hook)
}

pub(crate) fn removal_path_allowed(path: &std::path::Path) -> bool {
    use std::path::{Component, Path};
    if path
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return false;
    }
    if matches!(path.to_str(), Some("tasks/tasks.csv" | "tasks/habits.csv")) {
        return true;
    }
    let project =
        path.starts_with(Path::new("projects")) || path.starts_with(Path::new("archive/projects"));
    project
        && matches!(
            path.file_name().and_then(|name| name.to_str()),
            Some(".METADATA.json" | "notes.md")
        )
}

#[cfg(test)]
mod tests;

/// Inspection does not parse or recover a partially installed generation.
pub(crate) fn has_pending(workspace: &WorkspaceContext, owner: &TaskStoreOwner) -> Result<bool> {
    owner.verify(workspace)?;
    for policy in [TargetPolicy::ManagedTriage, TargetPolicy::TaskRemoval] {
        match fs::symlink_metadata(journal_path(workspace, policy)) {
            Ok(_) => return Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(false)
}
