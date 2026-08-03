//! Durable inactive task-schema replacement and crash recovery.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use super::{sync_parent, write_new};
use crate::workspace::WorkspaceId;

const JOURNAL_SCHEMA_VERSION: u32 = 1;
const REPLACEMENTS: [&str; 3] = ["tasks.csv", "habits.csv", "SCHEMA.json"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MigrationStep {
    BackupParentOpen(usize),
    BackupParentSync(usize),
    Stage(usize),
    Install(usize),
    Restore(usize),
    Commit,
    Committed,
}

#[derive(Debug)]
pub(super) struct FileChange {
    pub(super) name: &'static str,
    pub(super) before: Vec<u8>,
    pub(super) after: Vec<u8>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum JournalState {
    Prepared,
    Committed,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema_version: u32,
    workspace_id: String,
    workspace_root: PathBuf,
    state: JournalState,
    entries: Vec<String>,
}

pub(super) fn replace_group(
    workspace_root: &Path,
    backup_dir: &Path,
    workspace_id: WorkspaceId,
    changes: &[FileChange],
    hook: &mut impl FnMut(MigrationStep) -> std::io::Result<()>,
) -> Result<()> {
    let tasks_dir = workspace_root.join("tasks");
    let result = prepare(&tasks_dir, changes, hook);
    if let Err(error) = result {
        cleanup_artifacts(&tasks_dir)?;
        return Err(error);
    }
    if let Err(error) = write_journal(
        backup_dir,
        workspace_root,
        workspace_id,
        JournalState::Prepared,
    ) {
        cleanup_artifacts(&tasks_dir)?;
        return Err(error);
    }
    if let Err(error) = install(&tasks_dir, changes, hook) {
        return match rollback(workspace_root, backup_dir, hook) {
            Ok(()) => Err(error),
            Err(rollback_error) => Err(anyhow!(
                "{error:#}; task-schema rollback also failed: {rollback_error:#}"
            )),
        };
    }
    if let Err(error) = hook(MigrationStep::Commit) {
        let error = anyhow!("commit task-schema migration: {error}");
        return match rollback(workspace_root, backup_dir, hook) {
            Ok(()) => Err(error),
            Err(rollback_error) => Err(anyhow!(
                "{error:#}; task-schema rollback also failed: {rollback_error:#}"
            )),
        };
    }
    write_journal(
        backup_dir,
        workspace_root,
        workspace_id,
        JournalState::Committed,
    )?;
    hook(MigrationStep::Committed).context("finishing committed task-schema migration")?;
    finish_committed(&tasks_dir, backup_dir)
}

pub(super) fn recover_pending(
    workspace_root: &Path,
    backup_dir: &Path,
    workspace_id: WorkspaceId,
    hook: &mut impl FnMut(MigrationStep) -> std::io::Result<()>,
) -> Result<()> {
    let path = journal_path(backup_dir);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            cleanup_artifacts(&workspace_root.join("tasks"))?;
            cleanup_journal_temporary(backup_dir)?;
            return Ok(());
        }
        Err(error) => {
            return Err(error).with_context(|| {
                format!("reading task-schema transaction journal {}", path.display())
            });
        }
    };
    let journal: Journal = serde_json::from_slice(&bytes)
        .with_context(|| format!("parsing task-schema transaction journal {}", path.display()))?;
    validate_journal(&journal, workspace_root, workspace_id)?;
    match journal.state {
        JournalState::Prepared => rollback(workspace_root, backup_dir, hook),
        JournalState::Committed => finish_committed(&workspace_root.join("tasks"), backup_dir),
    }
}

pub(super) fn journal_path(backup_dir: &Path) -> PathBuf {
    backup_dir.join(".brain-task-schema-transaction.json")
}

fn prepare(
    tasks_dir: &Path,
    changes: &[FileChange],
    hook: &mut impl FnMut(MigrationStep) -> std::io::Result<()>,
) -> Result<()> {
    cleanup_artifacts(tasks_dir)?;
    for (index, change) in changes.iter().enumerate() {
        hook(MigrationStep::Stage(index))
            .with_context(|| format!("staging task-schema replacement for {}", change.name))?;
        let live = tasks_dir.join(change.name);
        let current = fs::read(&live)
            .with_context(|| format!("verifying task-schema input {}", live.display()))?;
        if current != change.before {
            bail!(
                "task-schema input changed before replacement: {}",
                live.display()
            );
        }
        let staged = staged_path(tasks_dir, change.name);
        write_new(&staged, &change.after)?;
        sync_parent(&staged)?;
    }
    Ok(())
}

fn install(
    tasks_dir: &Path,
    changes: &[FileChange],
    hook: &mut impl FnMut(MigrationStep) -> std::io::Result<()>,
) -> Result<()> {
    for (index, change) in changes.iter().enumerate() {
        let live = tasks_dir.join(change.name);
        hook(MigrationStep::Install(index))
            .with_context(|| format!("installing task-schema replacement {}", live.display()))?;
        fs::rename(staged_path(tasks_dir, change.name), &live)
            .with_context(|| format!("atomically replacing {}", live.display()))?;
        sync_parent(&live)?;
    }
    Ok(())
}

fn rollback(
    workspace_root: &Path,
    backup_dir: &Path,
    hook: &mut impl FnMut(MigrationStep) -> std::io::Result<()>,
) -> Result<()> {
    let tasks_dir = workspace_root.join("tasks");
    for (index, name) in REPLACEMENTS.into_iter().enumerate() {
        let live = tasks_dir.join(name);
        let backup = backup_dir.join("tasks").join(name);
        let restore = restore_path(&tasks_dir, name);
        hook(MigrationStep::Restore(index))
            .with_context(|| format!("restoring task-schema input {}", live.display()))?;
        remove_if_exists(&restore)?;
        let bytes = fs::read(&backup)
            .with_context(|| format!("reading task-schema rollback backup {}", backup.display()))?;
        write_new(&restore, &bytes)?;
        fs::rename(&restore, &live)
            .with_context(|| format!("restoring task-schema input {}", live.display()))?;
        sync_parent(&live)?;
    }
    clear_journal(backup_dir)?;
    cleanup_artifacts(&tasks_dir)?;
    Ok(())
}

fn finish_committed(tasks_dir: &Path, backup_dir: &Path) -> Result<()> {
    clear_journal(backup_dir)?;
    cleanup_artifacts(tasks_dir)
}

fn write_journal(
    backup_dir: &Path,
    workspace_root: &Path,
    workspace_id: WorkspaceId,
    state: JournalState,
) -> Result<()> {
    let journal = Journal {
        schema_version: JOURNAL_SCHEMA_VERSION,
        workspace_id: workspace_id.to_string(),
        workspace_root: workspace_root.to_path_buf(),
        state,
        entries: REPLACEMENTS.iter().map(|name| (*name).to_owned()).collect(),
    };
    let mut bytes = serde_json::to_vec_pretty(&journal)?;
    bytes.push(b'\n');
    let path = journal_path(backup_dir);
    let temporary = journal_temporary_path(backup_dir);
    remove_if_exists(&temporary)?;
    write_new(&temporary, &bytes)?;
    fs::rename(&temporary, &path)
        .with_context(|| format!("publishing task-schema journal {}", path.display()))?;
    sync_parent(&path)
}

fn validate_journal(
    journal: &Journal,
    workspace_root: &Path,
    workspace_id: WorkspaceId,
) -> Result<()> {
    if journal.schema_version != JOURNAL_SCHEMA_VERSION {
        bail!(
            "unsupported task-schema journal version {}",
            journal.schema_version
        );
    }
    if journal.workspace_id != workspace_id.to_string() || journal.workspace_root != workspace_root
    {
        bail!("task-schema journal belongs to a different workspace");
    }
    let entries = journal
        .entries
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let expected = REPLACEMENTS.into_iter().collect::<HashSet<_>>();
    if entries != expected || journal.entries.len() != REPLACEMENTS.len() {
        bail!("task-schema journal has unsafe or incomplete entries");
    }
    Ok(())
}

fn clear_journal(backup_dir: &Path) -> Result<()> {
    let path = journal_path(backup_dir);
    match fs::remove_file(&path) {
        Ok(()) => sync_parent(&path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => {
            Err(error).with_context(|| format!("clearing task-schema journal {}", path.display()))
        }
    }
}

fn cleanup_journal_temporary(backup_dir: &Path) -> Result<()> {
    remove_if_exists(&journal_temporary_path(backup_dir))
}

fn cleanup_artifacts(tasks_dir: &Path) -> Result<()> {
    for name in REPLACEMENTS {
        remove_if_exists(&staged_path(tasks_dir, name))?;
        remove_if_exists(&restore_path(tasks_dir, name))?;
    }
    Ok(())
}

fn remove_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("removing {}", path.display())),
    }
}

fn staged_path(tasks_dir: &Path, name: &str) -> PathBuf {
    tasks_dir.join(format!(".brain-task-schema-{name}.staged"))
}

fn restore_path(tasks_dir: &Path, name: &str) -> PathBuf {
    tasks_dir.join(format!(".brain-task-schema-{name}.restore"))
}

fn journal_temporary_path(backup_dir: &Path) -> PathBuf {
    backup_dir.join(".brain-task-schema-transaction.json.tmp")
}
