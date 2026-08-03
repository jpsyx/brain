//! Durable grouped replacement for triage config and its managed data.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

static NONCE: AtomicU64 = AtomicU64::new(0);
const JOURNAL_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub(super) struct FileChange {
    pub(super) path: PathBuf,
    pub(super) before: Option<Vec<u8>>,
    pub(super) after: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TransactionStep {
    Stage(usize),
    Install(usize),
    Restore(usize),
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
    existed: bool,
}

#[derive(Debug)]
struct Prepared {
    live: PathBuf,
    staged: PathBuf,
    backup: PathBuf,
    existed: bool,
}

pub(super) fn replace_group(root: &Path, changes: &[FileChange]) -> Result<()> {
    replace_group_with_hook(root, changes, |_| Ok(()))
}

pub(super) fn replace_group_with_hook(
    root: &Path,
    changes: &[FileChange],
    mut hook: impl FnMut(TransactionStep) -> std::io::Result<()>,
) -> Result<()> {
    recover_pending(root)?;
    let prepared = prepare(root, changes, &mut hook)?;
    if let Err(error) = write_journal(root, &prepared) {
        cleanup(&prepared);
        return Err(error);
    }
    for (index, change) in prepared.iter().enumerate() {
        if let Err(error) = install(change, index, &mut hook) {
            return rollback_after_error(root, &prepared, &mut hook, error);
        }
    }
    let journal = journal_path(root);
    if let Err(error) = fs::remove_file(&journal)
        .with_context(|| format!("committing triage transaction {}", journal.display()))
        .and_then(|()| sync_parent(&journal))
    {
        return rollback_after_error(root, &prepared, &mut hook, error);
    }
    cleanup(&prepared);
    Ok(())
}

pub(super) fn recover_pending(root: &Path) -> Result<()> {
    let path = journal_path(root);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
    };
    let journal: Journal =
        serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))?;
    if journal.schema_version != JOURNAL_SCHEMA_VERSION {
        bail!(
            "unsupported triage transaction schema {}",
            journal.schema_version
        );
    }
    let prepared = journal
        .entries
        .into_iter()
        .map(|entry| {
            for relative in [&entry.live, &entry.staged, &entry.backup] {
                if relative.is_absolute()
                    || relative
                        .components()
                        .any(|component| matches!(component, std::path::Component::ParentDir))
                {
                    bail!("invalid triage transaction path {}", relative.display());
                }
            }
            Ok(Prepared {
                live: root.join(entry.live),
                staged: root.join(entry.staged),
                backup: root.join(entry.backup),
                existed: entry.existed,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    rollback(root, &prepared, &mut |_| Ok(()))?;
    cleanup(&prepared);
    Ok(())
}

pub(super) fn journal_path(root: &Path) -> PathBuf {
    root.join(".config/.brain-triage-habits-transaction.json")
}

fn prepare(
    root: &Path,
    changes: &[FileChange],
    hook: &mut impl FnMut(TransactionStep) -> std::io::Result<()>,
) -> Result<Vec<Prepared>> {
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
    let mut prepared = Vec::with_capacity(changes.len());
    for (index, change) in changes.iter().enumerate() {
        let result = (|| {
            hook(TransactionStep::Stage(index))
                .with_context(|| format!("staging {}", change.path.display()))?;
            if !change.path.starts_with(root) {
                bail!(
                    "triage transaction path is outside workspace: {}",
                    change.path.display()
                );
            }
            let current = match fs::read(&change.path) {
                Ok(bytes) => Some(bytes),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("reading {}", change.path.display()));
                }
            };
            if current != change.before {
                bail!(
                    "{} changed during triage reconciliation",
                    change.path.display()
                );
            }
            let parent = change.path.parent().unwrap_or_else(|| Path::new("."));
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
            let name = change
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("data");
            let staged = parent.join(format!(
                ".{name}.brain-triage-{}-{nonce}-{index}.staged",
                std::process::id()
            ));
            let backup = parent.join(format!(
                ".{name}.brain-triage-{}-{nonce}-{index}.backup",
                std::process::id()
            ));
            write_new(&staged, &change.after)
                .with_context(|| format!("staging {}", change.path.display()))?;
            prepared.push(Prepared {
                live: change.path.clone(),
                staged,
                backup,
                existed: current.is_some(),
            });
            Ok(())
        })();
        if let Err(error) = result {
            cleanup(&prepared);
            return Err(error);
        }
    }
    Ok(prepared)
}

fn write_journal(root: &Path, prepared: &[Prepared]) -> Result<()> {
    let entries = prepared
        .iter()
        .map(|change| {
            Ok(JournalEntry {
                live: change.live.strip_prefix(root)?.to_path_buf(),
                staged: change.staged.strip_prefix(root)?.to_path_buf(),
                backup: change.backup.strip_prefix(root)?.to_path_buf(),
                existed: change.existed,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let mut bytes = serde_json::to_vec_pretty(&Journal {
        schema_version: JOURNAL_SCHEMA_VERSION,
        entries,
    })?;
    bytes.push(b'\n');
    let path = journal_path(root);
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension("json.tmp");
    if temporary.exists() {
        fs::remove_file(&temporary)?;
    }
    write_new(&temporary, &bytes)?;
    fs::rename(&temporary, &path)?;
    sync_parent(&path)
}

fn install(
    change: &Prepared,
    index: usize,
    hook: &mut impl FnMut(TransactionStep) -> std::io::Result<()>,
) -> Result<()> {
    hook(TransactionStep::Install(index))
        .with_context(|| format!("installing {}", change.live.display()))?;
    if change.existed {
        fs::rename(&change.live, &change.backup)
            .with_context(|| format!("backing up {}", change.live.display()))?;
    }
    fs::rename(&change.staged, &change.live)
        .with_context(|| format!("installing {}", change.live.display()))?;
    sync_parent(&change.live)
}

fn rollback_after_error(
    root: &Path,
    prepared: &[Prepared],
    hook: &mut impl FnMut(TransactionStep) -> std::io::Result<()>,
    error: anyhow::Error,
) -> Result<()> {
    match rollback(root, prepared, hook) {
        Ok(()) => {
            cleanup(prepared);
            Err(error)
        }
        Err(rollback_error) => Err(anyhow!(
            "{error:#}; rollback also failed: {rollback_error:#}"
        )),
    }
}

fn rollback(
    root: &Path,
    changes: &[Prepared],
    hook: &mut impl FnMut(TransactionStep) -> std::io::Result<()>,
) -> Result<()> {
    let mut errors = Vec::new();
    for (index, change) in changes.iter().enumerate().rev() {
        let needs_restore = if change.existed {
            change.backup.exists()
        } else {
            !change.staged.exists() && change.live.exists()
        };
        if !needs_restore {
            continue;
        }
        if let Err(error) = hook(TransactionStep::Restore(index)) {
            errors.push(format!("restore {}: {error}", change.live.display()));
            continue;
        }
        if change.live.exists()
            && let Err(error) = fs::remove_file(&change.live)
        {
            errors.push(format!("remove {}: {error}", change.live.display()));
            continue;
        }
        if change.existed
            && let Err(error) = fs::rename(&change.backup, &change.live)
        {
            errors.push(format!("restore {}: {error}", change.live.display()));
            continue;
        }
        if let Err(error) = sync_parent(&change.live) {
            errors.push(error.to_string());
        }
    }
    if !errors.is_empty() {
        return Err(anyhow!(errors.join("; ")));
    }
    let journal = journal_path(root);
    match fs::remove_file(&journal) {
        Ok(()) => sync_parent(&journal)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).with_context(|| format!("clearing {}", journal.display())),
    }
    Ok(())
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

fn sync_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

fn cleanup(changes: &[Prepared]) {
    for change in changes {
        let _ = fs::remove_file(&change.staged);
        let _ = fs::remove_file(&change.backup);
    }
}

#[cfg(test)]
mod tests;
