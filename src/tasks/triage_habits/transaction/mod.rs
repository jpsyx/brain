//! Authenticated, crash-safe grouped replacement for managed triage data.

mod artifacts;
mod journal;

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use self::artifacts::{cleanup_artifacts, describe, sync_parent, write_new};
use self::journal::{journal_path, publish_journal, recover_pending_locked};
use crate::tasks::store_lock::TaskStoreOwner;
use crate::workspace::WorkspaceContext;

pub(super) static NONCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub(super) struct FileChange {
    pub(super) path: PathBuf,
    pub(super) before: Option<Vec<u8>>,
    pub(super) after: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum JournalState {
    Preparing,
    Prepared,
    Committed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TransactionStep {
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
    RemoveJournal,
    SyncJournalRemoval,
}

#[derive(Debug, Clone)]
pub(super) struct Prepared {
    pub(super) live: PathBuf,
    pub(super) staged: PathBuf,
    pub(super) backup: PathBuf,
    pub(super) existed: bool,
    pub(super) before: Option<Vec<u8>>,
    pub(super) after: Vec<u8>,
}

pub(super) fn replace_group(
    workspace: &WorkspaceContext,
    owner: &TaskStoreOwner,
    changes: &[FileChange],
) -> Result<()> {
    replace_group_with_hook(workspace, owner, changes, |_| Ok(()))
}

pub(super) fn replace_group_with_hook(
    workspace: &WorkspaceContext,
    owner: &TaskStoreOwner,
    changes: &[FileChange],
    mut hook: impl FnMut(TransactionStep) -> std::io::Result<()>,
) -> Result<()> {
    owner.verify(workspace)?;
    recover_pending_locked(workspace, &mut hook)?;
    if changes.is_empty() {
        return Ok(());
    }
    let prepared = describe(workspace.root(), changes)?;
    publish_journal(workspace, &prepared, JournalState::Preparing, &mut hook)?;
    let result = transact(workspace, &prepared, &mut hook);
    if let Err(error) = result {
        return match recover_pending_locked(workspace, &mut hook) {
            Ok(()) => Err(error),
            Err(recovery) => Err(anyhow!("{error:#}; recovery also failed: {recovery:#}")),
        };
    }
    Ok(())
}

fn transact(
    workspace: &WorkspaceContext,
    prepared: &[Prepared],
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
    publish_journal(workspace, prepared, JournalState::Prepared, hook)?;
    for (index, change) in prepared.iter().enumerate() {
        hook(TransactionStep::Install(index))?;
        fs::rename(&change.staged, &change.live)
            .with_context(|| format!("atomically replacing {}", change.live.display()))?;
        hook(TransactionStep::SyncInstall(index))?;
        sync_parent(&change.live)?;
    }
    publish_journal(workspace, prepared, JournalState::Committed, hook)?;
    cleanup_artifacts(prepared, hook)?;
    journal::remove_journal(&journal_path(workspace.root()), hook)
}

pub(super) fn recover_pending(workspace: &WorkspaceContext, owner: &TaskStoreOwner) -> Result<()> {
    owner.verify(workspace)?;
    recover_pending_locked(workspace, &mut |_| Ok(()))
}

#[cfg(test)]
mod tests;
