//! Exact machine-local backup of portable cutover inputs.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

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
    AfterValidation,
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
    validate_existing_inventory(backup_dir)?;
    hook(
        Path::new(".config/config.json"),
        BackupWriteStep::AfterValidation,
    )
    .context("preparing migration backup destination")?;
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

fn validate_existing_inventory(backup_dir: &Path) -> Result<()> {
    validate_existing_directory(backup_dir)?;
    for (relative, _) in INVENTORY {
        let destination = backup_dir.join(relative);
        let parent = destination.parent().ok_or_else(|| {
            anyhow!(
                "migration backup destination has no parent: {}",
                destination.display()
            )
        })?;
        let relative_parent = parent.strip_prefix(backup_dir).with_context(|| {
            format!(
                "migration backup destination {} is outside {}",
                destination.display(),
                backup_dir.display()
            )
        })?;
        let mut current = backup_dir.to_path_buf();
        for component in relative_parent.components() {
            current.push(component);
            validate_existing_directory(&current)?;
        }
        match fs::symlink_metadata(&destination) {
            Ok(metadata) if metadata.file_type().is_symlink() => bail!(
                "migration backup destination must not be a symlink: {}",
                destination.display()
            ),
            Ok(metadata) if !metadata.is_file() => bail!(
                "migration backup destination must be a regular file: {}",
                destination.display()
            ),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "inspecting migration backup destination {}",
                        destination.display()
                    )
                });
            }
        }
    }
    Ok(())
}

fn validate_existing_directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => bail!(
            "migration backup directory must not be a symlink: {}",
            path.display()
        ),
        Ok(metadata) if !metadata.is_dir() => bail!(
            "migration backup directory component is not a directory: {}",
            path.display()
        ),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error)
            .with_context(|| format!("inspecting migration backup directory {}", path.display())),
    }
}

fn validate_destination(root: &Path, backup_base: &Path, backup_dir: &Path) -> Result<()> {
    if !root.is_absolute() || !backup_base.is_absolute() || !backup_dir.is_absolute() {
        bail!("migration backup paths must be absolute");
    }
    let root = resolve_path(root)?;
    let backup_base = resolve_path(backup_base)?;
    let backup_dir = resolve_path(backup_dir)?;
    if !backup_dir.starts_with(&backup_base) {
        bail!("migration backup directory must be below its selected workspace cache base");
    }
    if root.starts_with(&backup_dir) || backup_dir.starts_with(&root) {
        bail!("migration backup must be disjoint from the workspace tree");
    }
    Ok(())
}

fn resolve_path(path: &Path) -> Result<PathBuf> {
    let normalized = normalize_absolute(path)?;
    let mut existing = normalized.as_path();
    let mut missing = Vec::new();
    let canonical = loop {
        match fs::canonicalize(existing) {
            Ok(canonical) => break canonical,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let name = existing.file_name().ok_or_else(|| {
                    anyhow!("cannot resolve migration backup path {}", path.display())
                })?;
                missing.push(name.to_owned());
                existing = existing.parent().ok_or_else(|| {
                    anyhow!("cannot resolve migration backup path {}", path.display())
                })?;
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("resolving migration backup path {}", path.display())
                });
            }
        }
    };
    let mut resolved = canonical;
    for component in missing.into_iter().rev() {
        resolved.push(component);
    }
    normalize_absolute(&resolved)
}

fn normalize_absolute(path: &Path) -> Result<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(Path::new("/")),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    bail!(
                        "migration backup path escapes its filesystem root: {}",
                        path.display()
                    );
                }
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    if !normalized.is_absolute() {
        bail!(
            "migration backup paths must be absolute: {}",
            path.display()
        );
    }
    Ok(normalized)
}

fn ensure_directory(path: &Path) -> Result<()> {
    fs::create_dir_all(path)
        .with_context(|| format!("creating migration backup directory {}", path.display()))?;
    validate_existing_directory(path)?;
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
    let temporary = std::env::temp_dir().join(format!(".brain-backup-{}.tmp", WorkspaceId::new()));
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
        #[cfg(unix)]
        {
            use nix::fcntl::{OFlag, openat, renameat};
            use nix::sys::stat::Mode;
            use nix::unistd::{close, fsync};

            let directory = openat(
                None,
                parent,
                OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW,
                Mode::empty(),
            )
            .with_context(|| format!("opening migration backup parent {}", parent.display()))?;
            let publish = (|| {
                hook().context("publishing verified migration backup")?;
                let name = destination.file_name().ok_or_else(|| {
                    anyhow!(
                        "migration backup destination has no file name: {}",
                        destination.display()
                    )
                })?;
                renameat(None, &temporary, Some(directory), name).with_context(|| {
                    format!("publishing migration backup {}", destination.display())
                })?;
                fsync(directory).context("syncing migration backup parent")
            })();
            let close_result = close(directory);
            publish?;
            close_result.context("closing migration backup parent")
        }
        #[cfg(not(unix))]
        {
            hook().context("publishing verified migration backup")?;
            fs::rename(&temporary, destination).with_context(|| {
                format!("publishing migration backup {}", destination.display())
            })?;
            fs::File::open(parent)
                .and_then(|directory| directory.sync_all())
                .with_context(|| format!("syncing migration backup parent {}", parent.display()))
        }
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn preexisting_backup_base_symlink_into_workspace_is_rejected() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("workspace");
        fs::create_dir_all(&root).unwrap();
        let base = temporary.path().join("cache-link");
        symlink(&root, &base).unwrap();
        let backup = base.join("20260806T120000Z-pre-multi-workspace");

        let error = backup_portable_data(&root, &base, &backup).unwrap_err();

        assert!(error.to_string().contains("disjoint"), "{error:#}");
        assert!(!root.join("20260806T120000Z-pre-multi-workspace").exists());
    }

    #[cfg(unix)]
    #[test]
    fn preexisting_nested_backup_symlink_into_workspace_is_rejected() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("workspace");
        let tasks = root.join("tasks");
        let base = temporary.path().join("cache/migration-backups");
        let backup = base.join("20260806T120000Z-pre-multi-workspace");
        fs::create_dir_all(&tasks).unwrap();
        fs::create_dir_all(&backup).unwrap();
        for (name, bytes) in [
            ("tasks.csv", b"task_id\nT1\n".as_slice()),
            ("habits.csv", b"task_id\nH1\n".as_slice()),
            ("SCHEMA.json", b"{}\n".as_slice()),
        ] {
            fs::write(tasks.join(name), bytes).unwrap();
        }
        symlink(&tasks, backup.join("tasks")).unwrap();

        let error = backup_portable_data(&root, &base, &backup).unwrap_err();

        assert!(error.to_string().contains("symlink"), "{error:#}");
        assert_eq!(fs::read(tasks.join("tasks.csv")).unwrap(), b"task_id\nT1\n");
    }

    #[test]
    fn preexisting_nested_backup_file_component_is_rejected() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("workspace");
        let tasks = root.join("tasks");
        let base = temporary.path().join("cache/migration-backups");
        let backup = base.join("20260806T120000Z-pre-multi-workspace");
        fs::create_dir_all(&tasks).unwrap();
        fs::create_dir_all(&backup).unwrap();
        for (name, bytes) in [
            ("tasks.csv", b"task_id\nT1\n".as_slice()),
            ("habits.csv", b"task_id\nH1\n".as_slice()),
            ("SCHEMA.json", b"{}\n".as_slice()),
        ] {
            fs::write(tasks.join(name), bytes).unwrap();
        }
        fs::write(backup.join("tasks"), b"not a directory\n").unwrap();

        let error = backup_portable_data(&root, &base, &backup).unwrap_err();

        assert!(error.to_string().contains("not a directory"), "{error:#}");
        assert_eq!(fs::read(tasks.join("tasks.csv")).unwrap(), b"task_id\nT1\n");
    }

    #[cfg(unix)]
    #[test]
    fn backup_publish_rejects_parent_replacement_after_validation() {
        use std::cell::Cell;
        use std::os::unix::fs::symlink;

        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("workspace");
        let root_config = root.join(".config");
        let root_tasks = root.join("tasks");
        let base = temporary.path().join("cache/migration-backups");
        let backup = base.join("20260806T120000Z-pre-multi-workspace");
        fs::create_dir_all(&root_config).unwrap();
        fs::create_dir_all(&root_tasks).unwrap();
        fs::create_dir_all(&backup).unwrap();
        fs::write(root_config.join("config.json"), b"portable-config\n").unwrap();
        let escaped_temp = Cell::new(false);

        let error = backup_portable_data_with_hook(&root, &base, &backup, |relative, step| {
            if relative == Path::new(".config/config.json")
                && step == BackupWriteStep::BeforePublish
            {
                fs::remove_dir_all(backup.join(".config"))?;
                symlink(&root_tasks, backup.join(".config"))?;
                escaped_temp.set(fs::read_dir(&root_tasks)?.flatten().any(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".config.json.")
                }));
                return Err(std::io::Error::other("stop after destination observation"));
            }
            Ok(())
        })
        .unwrap_err();

        assert!(
            format!("{error:#}").contains("stop after destination observation"),
            "{error:#}"
        );
        assert!(!escaped_temp.get());
    }

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
