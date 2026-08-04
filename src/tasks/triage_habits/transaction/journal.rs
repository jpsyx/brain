//! Authenticated journal publication, validation, recovery, and rollback.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::artifacts::{
    artifact_paths, cleanup_artifacts, relative, remove_if_exists, sync_parent,
    transaction_id_from_artifact, validate_live, validate_relative, validate_transaction_id,
    write_new,
};
use super::{JournalState, NONCE, Prepared, TransactionStep};
use crate::workspace::WorkspaceContext;

const JOURNAL_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema_version: u32,
    workspace_id: String,
    workspace_root: PathBuf,
    transaction_id: String,
    state: JournalState,
    entries: Vec<JournalEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JournalEntry {
    live: PathBuf,
    staged: PathBuf,
    backup: PathBuf,
    existed: bool,
}

pub(super) fn publish_journal(
    workspace: &WorkspaceContext,
    prepared: &[Prepared],
    state: JournalState,
    hook: &mut impl FnMut(TransactionStep) -> std::io::Result<()>,
) -> Result<()> {
    let root = workspace.root();
    let transaction_id = transaction_id_from_artifact(&prepared[0].staged)?;
    let entries = prepared
        .iter()
        .map(|change| {
            Ok(JournalEntry {
                live: relative(root, &change.live)?,
                staged: relative(root, &change.staged)?,
                backup: relative(root, &change.backup)?,
                existed: change.existed,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let mut bytes = serde_json::to_vec_pretty(&Journal {
        schema_version: JOURNAL_SCHEMA_VERSION,
        workspace_id: workspace.id().to_string(),
        workspace_root: root.to_path_buf(),
        transaction_id,
        state,
        entries,
    })?;
    bytes.push(b'\n');
    let path = journal_path(root);
    fs::create_dir_all(path.parent().expect("journal has parent"))?;
    let temporary = path.with_extension(format!(
        "json.pending-{}-{}",
        std::process::id(),
        NONCE.fetch_add(1, Ordering::Relaxed)
    ));
    hook(TransactionStep::PublishJournalWrite(state))?;
    write_new(&temporary, &bytes)?;
    hook(TransactionStep::PublishJournalRename(state))?;
    fs::rename(&temporary, &path)?;
    hook(TransactionStep::PublishJournalSync(state))?;
    sync_parent(&path)
}

pub(super) fn recover_pending_locked(
    workspace: &WorkspaceContext,
    hook: &mut impl FnMut(TransactionStep) -> std::io::Result<()>,
) -> Result<()> {
    let path = journal_path(workspace.root());
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
    };
    let journal: Journal = serde_json::from_slice(&bytes)
        .with_context(|| format!("parsing authenticated journal {}", path.display()))?;
    let prepared = validate_journal(workspace, &journal)?;
    match journal.state {
        JournalState::Prepared => rollback(&prepared, hook)?,
        JournalState::Preparing | JournalState::Committed => {
            cleanup_artifacts(&prepared, hook)?;
        }
    }
    remove_journal(&path, hook)
}

fn validate_journal(workspace: &WorkspaceContext, journal: &Journal) -> Result<Vec<Prepared>> {
    if journal.schema_version != JOURNAL_SCHEMA_VERSION
        || journal.workspace_id != workspace.id().to_string()
        || journal.workspace_root != workspace.root()
    {
        bail!("triage transaction journal does not belong to selected workspace");
    }
    validate_transaction_id(&journal.transaction_id)?;
    let mut seen = BTreeSet::new();
    journal
        .entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            validate_live(workspace.root(), &entry.live)?;
            let live = workspace.root().join(&entry.live);
            let (staged, backup) = artifact_paths(&live, &journal.transaction_id, index)?;
            if workspace.root().join(&entry.staged) != staged
                || workspace.root().join(&entry.backup) != backup
            {
                bail!("triage transaction artifact is not the exact sibling of its live target");
            }
            for path in [&entry.live, &entry.staged, &entry.backup] {
                validate_relative(path)?;
                if !seen.insert(path.clone()) {
                    bail!("duplicate triage transaction path");
                }
            }
            Ok(Prepared {
                live,
                staged,
                backup,
                existed: entry.existed,
                before: None,
                after: Vec::new(),
            })
        })
        .collect()
}

fn rollback(
    prepared: &[Prepared],
    hook: &mut impl FnMut(TransactionStep) -> std::io::Result<()>,
) -> Result<()> {
    for (index, change) in prepared.iter().enumerate().rev() {
        hook(TransactionStep::Restore(index))?;
        if change.existed {
            let bytes = fs::read(&change.backup).with_context(|| {
                format!("reading authenticated backup {}", change.backup.display())
            })?;
            let restore = change.backup.with_extension("backup.restore");
            write_new(&restore, &bytes)?;
            fs::rename(&restore, &change.live)?;
        } else if !change.staged.exists() {
            remove_if_exists(&change.live)?;
        }
        sync_parent(&change.live)?;
    }
    cleanup_artifacts(prepared, hook)
}

pub(super) fn remove_journal(
    path: &Path,
    hook: &mut impl FnMut(TransactionStep) -> std::io::Result<()>,
) -> Result<()> {
    hook(TransactionStep::RemoveJournal)?;
    remove_if_exists(path)?;
    hook(TransactionStep::SyncJournalRemoval)?;
    sync_parent(path)
}

pub(super) fn journal_path(root: &Path) -> PathBuf {
    root.join(".config/.brain-triage-habits-transaction.json")
}
