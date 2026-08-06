//! Exact machine-local backup of portable cutover inputs.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};

use crate::workspace::WorkspaceId;

const INVENTORY: [(&str, bool); 9] = [
    (".config/config.json", false),
    (".config/personalization.json", false),
    (".config/users.json", false),
    (".config/workspace.json", false),
    ("tasks/tasks.csv", true),
    ("tasks/habits.csv", true),
    ("tasks/.tasks_next_id", false),
    ("tasks/.habits_next_id", false),
    ("tasks/SCHEMA.json", true),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackupWriteStep {
    BeforePublish,
}

/// Derive one injected-timestamp backup directory below the selected cache.
pub fn backup_directory(base: &Path, timestamp: &str) -> Result<PathBuf> {
    if timestamp.is_empty()
        || !timestamp
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("migration backup timestamp is not a safe path component");
    }
    Ok(base.join(format!("{timestamp}-pre-multi-workspace")))
}

/// Copy the exact portable migration inventory without reading other files.
pub fn backup_portable_data(root: &Path, backup_base: &Path, backup_dir: &Path) -> Result<()> {
    backup_portable_data_with_hook(root, backup_base, backup_dir, |_, _| Ok(()))
}

fn backup_portable_data_with_hook(
    root: &Path,
    backup_base: &Path,
    backup_dir: &Path,
    mut hook: impl FnMut(&Path, BackupWriteStep) -> std::io::Result<()>,
) -> Result<()> {
    validate_destination(root, backup_base, backup_dir)?;
    ensure_directory(backup_base)?;
    ensure_directory(backup_dir)?;
    for (relative, required) in INVENTORY {
        let source = root.join(relative);
        let bytes = match fs::read(&source) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && !required => continue,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("reading migration backup input {}", source.display())
                });
            }
        };
        let destination = backup_dir.join(relative);
        let parent = destination.parent().ok_or_else(|| {
            anyhow!(
                "migration backup destination has no parent: {}",
                destination.display()
            )
        })?;
        ensure_directory(parent)?;
        write_verified_with_hook(&destination, &bytes, || {
            hook(Path::new(relative), BackupWriteStep::BeforePublish)
        })?;
    }
    Ok(())
}

fn validate_destination(root: &Path, backup_base: &Path, backup_dir: &Path) -> Result<()> {
    if !root.is_absolute() || !backup_base.is_absolute() || !backup_dir.is_absolute() {
        bail!("migration backup paths must be absolute");
    }
    if !backup_dir.starts_with(backup_base) {
        bail!("migration backup directory must be below its selected workspace cache base");
    }
    if root.starts_with(backup_dir) || backup_dir.starts_with(root) {
        bail!("migration backup must be disjoint from the workspace tree");
    }
    Ok(())
}

fn ensure_directory(path: &Path) -> Result<()> {
    fs::create_dir_all(path)
        .with_context(|| format!("creating migration backup directory {}", path.display()))?;
    let parent = path.parent().ok_or_else(|| {
        anyhow!(
            "migration backup directory has no parent: {}",
            path.display()
        )
    })?;
    fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .with_context(|| {
            format!(
                "syncing migration backup directory parent {}",
                parent.display()
            )
        })
}

fn write_verified_with_hook(
    destination: &Path,
    bytes: &[u8],
    hook: impl FnOnce() -> std::io::Result<()>,
) -> Result<()> {
    if destination.exists() {
        let existing = fs::read(destination).with_context(|| {
            format!(
                "reading existing migration backup {}",
                destination.display()
            )
        })?;
        if existing != bytes {
            bail!(
                "migration backup already exists with different bytes: {}",
                destination.display()
            );
        }
        return Ok(());
    }
    let parent = destination.parent().ok_or_else(|| {
        anyhow!(
            "migration backup destination has no parent: {}",
            destination.display()
        )
    })?;
    let temporary = destination.with_file_name(format!(
        ".{}.{}.tmp",
        destination
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("backup"),
        WorkspaceId::new()
    ));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary).with_context(|| {
            format!(
                "creating migration backup temporary {}",
                temporary.display()
            )
        })?;
        file.write_all(bytes).with_context(|| {
            format!("writing migration backup temporary {}", temporary.display())
        })?;
        file.sync_all().with_context(|| {
            format!("syncing migration backup temporary {}", temporary.display())
        })?;
        if fs::read(&temporary).with_context(|| {
            format!(
                "verifying migration backup temporary {}",
                temporary.display()
            )
        })? != bytes
        {
            bail!("migration backup temporary verification failed");
        }
        hook().context("publishing verified migration backup")?;
        fs::rename(&temporary, destination)
            .with_context(|| format!("publishing migration backup {}", destination.display()))?;
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .with_context(|| format!("syncing migration backup parent {}", parent.display()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_publish_failure_leaves_live_data_and_failed_destination_unchanged() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("workspace");
        let base = temporary.path().join("cache/migration-backups");
        let backup = base.join("20260806T120000Z-pre-multi-workspace");
        let files = [
            (
                "tasks/tasks.csv",
                b"task_id,task_name\nT1,Plan\n".as_slice(),
            ),
            (
                "tasks/habits.csv",
                b"task_id,task_name\nH1,Walk\n".as_slice(),
            ),
            ("tasks/SCHEMA.json", b"{}\n".as_slice()),
        ];
        for (relative, bytes) in files {
            let path = root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }

        let error = backup_portable_data_with_hook(&root, &base, &backup, |relative, step| {
            if relative == Path::new("tasks/habits.csv") && step == BackupWriteStep::BeforePublish {
                return Err(std::io::Error::other("injected backup publish failure"));
            }
            Ok(())
        })
        .unwrap_err();

        assert!(format!("{error:#}").contains("injected backup publish failure"));
        for (relative, bytes) in files {
            assert_eq!(fs::read(root.join(relative)).unwrap(), bytes);
        }
        assert!(!backup.join("tasks/habits.csv").exists());
        assert!(
            fs::read_dir(backup.join("tasks"))
                .unwrap()
                .flatten()
                .all(|entry| !entry.file_name().to_string_lossy().contains("habits.csv"))
        );
    }
}
