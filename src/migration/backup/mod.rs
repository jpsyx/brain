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
mod tests;
