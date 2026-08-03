//! Authenticated, crash-safe grouped replacement for managed triage data.

use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::tasks::store_lock::TaskStoreOwner;
use crate::workspace::WorkspaceContext;

static NONCE: AtomicU64 = AtomicU64::new(0);
const JOURNAL_SCHEMA_VERSION: u32 = 2;

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

#[derive(Debug, Clone)]
struct Prepared {
    live: PathBuf,
    staged: PathBuf,
    backup: PathBuf,
    existed: bool,
    before: Option<Vec<u8>>,
    after: Vec<u8>,
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
    remove_journal(&journal_path(workspace.root()), hook)
}

pub(super) fn recover_pending(workspace: &WorkspaceContext, owner: &TaskStoreOwner) -> Result<()> {
    owner.verify(workspace)?;
    recover_pending_locked(workspace, &mut |_| Ok(()))
}

fn recover_pending_locked(
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

pub(super) fn journal_path(root: &Path) -> PathBuf {
    root.join(".config/.brain-triage-habits-transaction.json")
}

fn describe(root: &Path, changes: &[FileChange]) -> Result<Vec<Prepared>> {
    let transaction_id = format!(
        "{}-{}",
        std::process::id(),
        NONCE.fetch_add(1, Ordering::Relaxed)
    );
    let mut seen = BTreeSet::new();
    changes
        .iter()
        .enumerate()
        .map(|(index, change)| {
            let relative = relative(root, &change.path)?;
            validate_live(root, &relative)?;
            if !seen.insert(relative) {
                bail!(
                    "duplicate triage transaction target {}",
                    change.path.display()
                );
            }
            let current = read_optional(&change.path)?;
            if current != change.before {
                bail!(
                    "{} changed before task ownership was acquired",
                    change.path.display()
                );
            }
            let (staged, backup) = artifact_paths(&change.path, &transaction_id, index)?;
            Ok(Prepared {
                live: change.path.clone(),
                staged,
                backup,
                existed: current.is_some(),
                before: current,
                after: change.after.clone(),
            })
        })
        .collect()
}

fn publish_journal(
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

fn cleanup_artifacts(
    prepared: &[Prepared],
    hook: &mut impl FnMut(TransactionStep) -> std::io::Result<()>,
) -> Result<()> {
    for (index, change) in prepared.iter().enumerate() {
        hook(TransactionStep::CleanupStaged(index))?;
        remove_if_exists(&change.staged)?;
        hook(TransactionStep::CleanupBackup(index))?;
        remove_if_exists(&change.backup)?;
        sync_parent(&change.live)?;
    }
    Ok(())
}

fn remove_journal(
    path: &Path,
    hook: &mut impl FnMut(TransactionStep) -> std::io::Result<()>,
) -> Result<()> {
    hook(TransactionStep::RemoveJournal)?;
    remove_if_exists(path)?;
    hook(TransactionStep::SyncJournalRemoval)?;
    sync_parent(path)
}

fn validate_live(root: &Path, path: &Path) -> Result<()> {
    validate_relative(path)?;
    let allowed = path == Path::new(".config/config.json")
        || matches!(
            path.to_str(),
            Some(
                "tasks/tasks.csv"
                    | "tasks/habits.csv"
                    | "tasks/.habits_next_id"
                    | "tasks/.tasks_next_id"
            )
        )
        || (path.starts_with("projects")
            && path.file_name().and_then(|name| name.to_str()) == Some(".METADATA.json"))
        || (path.parent() == Some(Path::new("tasks"))
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(is_derived_name));
    if !allowed {
        bail!("unapproved triage transaction target {}", path.display());
    }
    reject_symlinks(root, path)
}

fn is_derived_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    !Path::new(&lower)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("csv"))
        && (lower.contains("agenda") || lower.contains("index") || lower.contains("lookup"))
}

fn reject_symlinks(root: &Path, path: &Path) -> Result<()> {
    let mut current = root.to_path_buf();
    for component in path.components() {
        let Component::Normal(part) = component else {
            continue;
        };
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!("symlink in triage transaction path {}", current.display())
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(error).with_context(|| format!("inspecting {}", current.display()));
            }
        }
    }
    Ok(())
}

fn artifact_paths(live: &Path, transaction_id: &str, index: usize) -> Result<(PathBuf, PathBuf)> {
    let parent = live
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent", live.display()))?;
    let name = live
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("non-UTF-8 transaction target"))?;
    let stem = format!(".{name}.brain-triage-{transaction_id}-{index}");
    Ok((
        parent.join(format!("{stem}.staged")),
        parent.join(format!("{stem}.backup")),
    ))
}

fn transaction_id_from_artifact(path: &Path) -> Result<String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("invalid artifact"))?;
    let tail = name
        .split_once(".brain-triage-")
        .ok_or_else(|| anyhow!("invalid artifact"))?
        .1;
    let tail = tail
        .strip_suffix(".staged")
        .ok_or_else(|| anyhow!("invalid artifact"))?;
    Ok(tail
        .rsplit_once('-')
        .ok_or_else(|| anyhow!("invalid artifact"))?
        .0
        .to_owned())
}

fn validate_transaction_id(value: &str) -> Result<()> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'-')
    {
        bail!("invalid transaction id");
    }
    Ok(())
}

fn relative(root: &Path, path: &Path) -> Result<PathBuf> {
    path.strip_prefix(root)
        .map(Path::to_path_buf)
        .with_context(|| format!("{} is outside {}", path.display(), root.display()))
}

fn validate_relative(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("unsafe triage transaction path {}", path.display());
    }
    Ok(())
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
    }
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn remove_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("removing {}", path.display())),
    }
}

fn sync_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
