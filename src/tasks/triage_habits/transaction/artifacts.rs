//! Transaction target authentication and durable artifact filesystem helpers.

use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::Ordering;

use anyhow::{Context, Result, anyhow, bail};

use super::{FileChange, NONCE, Prepared, TransactionStep};

pub(super) fn describe(root: &Path, changes: &[FileChange]) -> Result<Vec<Prepared>> {
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

pub(super) fn cleanup_artifacts(
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

pub(super) fn validate_live(root: &Path, path: &Path) -> Result<()> {
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

pub(super) fn artifact_paths(
    live: &Path,
    transaction_id: &str,
    index: usize,
) -> Result<(PathBuf, PathBuf)> {
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

pub(super) fn transaction_id_from_artifact(path: &Path) -> Result<String> {
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

pub(super) fn validate_transaction_id(value: &str) -> Result<()> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'-')
    {
        bail!("invalid transaction id");
    }
    Ok(())
}

pub(super) fn relative(root: &Path, path: &Path) -> Result<PathBuf> {
    path.strip_prefix(root)
        .map(Path::to_path_buf)
        .with_context(|| format!("{} is outside {}", path.display(), root.display()))
}

pub(super) fn validate_relative(path: &Path) -> Result<()> {
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

pub(super) fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
    }
}

pub(super) fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
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

pub(super) fn remove_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("removing {}", path.display())),
    }
}

pub(super) fn sync_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}
