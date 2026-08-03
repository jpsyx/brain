//! Durable multi-file portable-user transactions and crash recovery.

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::UsersError;

mod files;

use files::{
    cleanup_orphans, cleanup_prepared, file_mode, io_error, relative_path, restore_path,
    sibling_path, sync_parent, transaction_error, transaction_nonce, validate_relative, write_new,
};

const JOURNAL_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub(crate) struct FileChange {
    pub(crate) path: PathBuf,
    before: Vec<u8>,
    after: Vec<u8>,
}

impl FileChange {
    pub(crate) fn new(path: PathBuf, before: Vec<u8>, after: Vec<u8>) -> Self {
        Self {
            path,
            before,
            after,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TransactionStep {
    Stage(usize),
    Install(usize),
    Restore(usize),
    RollbackCleanup,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema_version: u32,
    entries: Vec<JournalEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JournalEntry {
    live: PathBuf,
    staged: PathBuf,
    backup: PathBuf,
}

#[derive(Debug)]
struct PreparedChange {
    live: PathBuf,
    staged: PathBuf,
    backup: PathBuf,
    before: Vec<u8>,
    after: Vec<u8>,
    mode: u32,
}

pub(crate) fn replace_group(
    root: &Path,
    lock_path: &Path,
    changes: Vec<FileChange>,
) -> Result<(), UsersError> {
    replace_group_with_hook(root, lock_path, changes, |_| Ok(()))
}

pub(super) fn replace_group_with_hook(
    root: &Path,
    lock_path: &Path,
    changes: Vec<FileChange>,
    mut hook: impl FnMut(TransactionStep) -> std::io::Result<()>,
) -> Result<(), UsersError> {
    with_lock(lock_path, || {
        recover_pending_locked(root)?;
        let prepared = prepare(root, changes, &mut hook)?;
        if let Err(error) = publish_and_install(root, &prepared, &mut hook) {
            return match rollback(root, &prepared, &mut hook) {
                Ok(()) => Err(error),
                Err(rollback_error) => Err(transaction_error(format!(
                    "{error}; rollback also failed: {rollback_error}"
                ))),
            };
        }
        finish_commit(root, &prepared, &mut hook)
    })
}

pub(super) fn recover_pending(root: &Path, lock_path: &Path) -> Result<(), UsersError> {
    with_lock(lock_path, || recover_pending_locked(root))
}

pub(super) fn journal_path(root: &Path) -> PathBuf {
    root.join(".config/.brain-user-transaction.json")
}

fn with_lock<T>(
    lock_path: &Path,
    action: impl FnOnce() -> Result<T, UsersError>,
) -> Result<T, UsersError> {
    let parent = lock_path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|error| io_error("create user transaction lock directory", parent, &error))?;
    let connection = Connection::open(lock_path)
        .map_err(|error| transaction_error(format!("open {}: {error}", lock_path.display())))?;
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|error| transaction_error(format!("configure user transaction lock: {error}")))?;
    connection
        .execute_batch("PRAGMA journal_mode = OFF; BEGIN IMMEDIATE")
        .map_err(|error| transaction_error(format!("acquire user transaction lock: {error}")))?;
    action()
}

fn prepare(
    root: &Path,
    changes: Vec<FileChange>,
    hook: &mut impl FnMut(TransactionStep) -> std::io::Result<()>,
) -> Result<Vec<PreparedChange>, UsersError> {
    let nonce = transaction_nonce();
    let mut prepared = Vec::with_capacity(changes.len());
    for (index, change) in changes.into_iter().enumerate() {
        let result = (|| {
            hook(TransactionStep::Stage(index))
                .map_err(|error| io_error("stage user transaction", &change.path, &error))?;
            let relative = relative_path(root, &change.path)?;
            let current = fs::read(&change.path)
                .map_err(|error| io_error("verify user transaction input", &change.path, &error))?;
            if current != change.before {
                return Err(transaction_error(format!(
                    "{} changed before the transaction lock was acquired",
                    change.path.display()
                )));
            }
            let mode = file_mode(&change.path)?;
            let staged = sibling_path(&change.path, &nonce, index, "staged")?;
            let backup = sibling_path(&change.path, &nonce, index, "backup")?;
            prepared.push(PreparedChange {
                live: relative,
                staged: relative_path(root, &staged)?,
                backup: relative_path(root, &backup)?,
                before: change.before,
                after: change.after,
                mode,
            });
            let item = prepared.last().expect("prepared change was just pushed");
            write_new(&root.join(&item.staged), &item.after, item.mode)?;
            write_new(&root.join(&item.backup), &item.before, item.mode)?;
            sync_parent(&root.join(&item.live));
            Ok(())
        })();
        if let Err(error) = result {
            cleanup_prepared(root, &prepared);
            return Err(error);
        }
    }
    Ok(prepared)
}

fn publish_and_install(
    root: &Path,
    prepared: &[PreparedChange],
    hook: &mut impl FnMut(TransactionStep) -> std::io::Result<()>,
) -> Result<(), UsersError> {
    write_journal(root, prepared)?;
    for (index, change) in prepared.iter().enumerate() {
        hook(TransactionStep::Install(index)).map_err(|error| {
            io_error("install user transaction", &root.join(&change.live), &error)
        })?;
        fs::rename(root.join(&change.staged), root.join(&change.live)).map_err(|error| {
            io_error(
                "replace user transaction file",
                &root.join(&change.live),
                &error,
            )
        })?;
        sync_parent(&root.join(&change.live));
    }
    Ok(())
}

fn write_journal(root: &Path, prepared: &[PreparedChange]) -> Result<(), UsersError> {
    let journal = Journal {
        schema_version: JOURNAL_SCHEMA_VERSION,
        entries: prepared
            .iter()
            .map(|change| JournalEntry {
                live: change.live.clone(),
                staged: change.staged.clone(),
                backup: change.backup.clone(),
            })
            .collect(),
    };
    let mut bytes = serde_json::to_vec_pretty(&journal)
        .map_err(|error| transaction_error(format!("serialize user transaction: {error}")))?;
    bytes.push(b'\n');
    let path = journal_path(root);
    let temporary = path.with_extension("json.tmp");
    if temporary.exists() {
        fs::remove_file(&temporary).map_err(|error| {
            io_error("remove stale user transaction journal", &temporary, &error)
        })?;
    }
    write_new(&temporary, &bytes, 0o600)?;
    fs::rename(&temporary, &path)
        .map_err(|error| io_error("publish user transaction journal", &path, &error))?;
    sync_parent(&path);
    Ok(())
}

fn finish_commit(
    root: &Path,
    prepared: &[PreparedChange],
    hook: &mut impl FnMut(TransactionStep) -> std::io::Result<()>,
) -> Result<(), UsersError> {
    let journal = journal_path(root);
    if let Err(error) = fs::remove_file(&journal) {
        let original = io_error("commit user transaction", &journal, &error);
        return match rollback(root, prepared, hook) {
            Ok(()) => Err(original),
            Err(rollback_error) => Err(transaction_error(format!(
                "{original}; rollback also failed: {rollback_error}"
            ))),
        };
    }
    sync_parent(&journal);
    cleanup_prepared(root, prepared);
    Ok(())
}

fn recover_pending_locked(root: &Path) -> Result<(), UsersError> {
    let path = journal_path(root);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            cleanup_orphans(root)?;
            return Ok(());
        }
        Err(error) => return Err(io_error("read user transaction journal", &path, &error)),
    };
    let journal: Journal = serde_json::from_slice(&bytes)
        .map_err(|error| transaction_error(format!("parse {}: {error}", path.display())))?;
    if journal.schema_version != JOURNAL_SCHEMA_VERSION {
        return Err(transaction_error(format!(
            "unsupported journal schema {}",
            journal.schema_version
        )));
    }
    let prepared = journal
        .entries
        .into_iter()
        .map(|entry| {
            validate_relative(&entry.live)?;
            validate_relative(&entry.staged)?;
            validate_relative(&entry.backup)?;
            Ok(PreparedChange {
                live: entry.live,
                staged: entry.staged,
                backup: entry.backup,
                before: Vec::new(),
                after: Vec::new(),
                mode: 0,
            })
        })
        .collect::<Result<Vec<_>, UsersError>>()?;
    rollback(root, &prepared, &mut |_| Ok(()))
}

fn rollback(
    root: &Path,
    prepared: &[PreparedChange],
    hook: &mut impl FnMut(TransactionStep) -> std::io::Result<()>,
) -> Result<(), UsersError> {
    let mut failures = Vec::new();
    for (index, change) in prepared.iter().enumerate() {
        let backup = root.join(&change.backup);
        let live = root.join(&change.live);
        let restore = restore_path(&backup);
        let result = (|| {
            hook(TransactionStep::Restore(index))
                .map_err(|error| io_error("restore user transaction", &live, &error))?;
            if restore.exists() {
                fs::remove_file(&restore).map_err(|error| {
                    io_error("remove stale user transaction restore", &restore, &error)
                })?;
            }
            let bytes = fs::read(&backup)
                .map_err(|error| io_error("read user transaction backup", &backup, &error))?;
            let mode = file_mode(&backup)?;
            write_new(&restore, &bytes, mode)?;
            fs::rename(&restore, &live)
                .map_err(|error| io_error("restore user transaction file", &live, &error))?;
            sync_parent(&live);
            Ok::<(), UsersError>(())
        })();
        if let Err(error) = result {
            let _ = fs::remove_file(&restore);
            failures.push(error.to_string());
        }
    }
    if !failures.is_empty() {
        return Err(transaction_error(failures.join("; ")));
    }
    let journal = journal_path(root);
    match fs::remove_file(&journal) {
        Ok(()) => sync_parent(&journal),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error("clear user transaction journal", &journal, &error)),
    }
    hook(TransactionStep::RollbackCleanup)
        .map_err(|error| io_error("finish rollback cleanup", &journal, &error))?;
    cleanup_prepared(root, prepared);
    Ok(())
}
