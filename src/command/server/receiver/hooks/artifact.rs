use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Whether a lifecycle artifact has to be replaced at all. Pure.
///
/// Every ordinary Brain launch reinstalls these artifacts, and replacing a byte-identical
/// file still gives it a new mtime — which trips Brain's own filesystem watcher
/// and uploads an unchanged hook script on every single startup. Comparing first
/// makes reinstallation genuinely idempotent on disk.
#[must_use]
pub(super) fn needs_rewrite(existing: Option<&[u8]>, contents: &str) -> bool {
    existing != Some(contents.as_bytes())
}

pub(super) fn write_static_file(
    destination: &Path,
    write_destination: &Path,
    contents: &str,
    mode: u32,
) -> Result<()> {
    if !needs_rewrite(std::fs::read(write_destination).ok().as_deref(), contents) {
        return Ok(());
    }
    let parent = write_destination.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .with_context(|| format!("create lifecycle directory {}", parent.display()))?;
    let file_name = write_destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("artifact");
    let temporary = parent.join(format!(".{file_name}.tmp-{}", uuid::Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    let mut file = options
        .open(&temporary)
        .with_context(|| format!("create lifecycle temporary {}", temporary.display()))?;
    let result = (|| {
        file.write_all(contents.as_bytes())
            .with_context(|| format!("write lifecycle temporary {}", temporary.display()))?;
        file.sync_all()
            .with_context(|| format!("sync lifecycle temporary {}", temporary.display()))?;
        drop(file);
        std::fs::rename(&temporary, write_destination).with_context(|| {
            format!(
                "replace lifecycle artifact {} from {}",
                destination.display(),
                temporary.display()
            )
        })?;
        if let Ok(directory) = std::fs::File::open(parent) {
            let _ = directory.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

fn resolve_write_destination(path: &Path) -> Result<PathBuf> {
    let mut current = path.to_path_buf();
    let mut visited = std::collections::BTreeSet::new();
    for _ in 0..64 {
        let metadata = match std::fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(current),
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("inspect lifecycle path {}", current.display()));
            }
        };
        if !metadata.file_type().is_symlink() {
            return Ok(current);
        }
        anyhow::ensure!(
            visited.insert(current.clone()),
            "symlink cycle while installing {}",
            path.display()
        );
        let target = std::fs::read_link(&current)
            .with_context(|| format!("read lifecycle symlink {}", current.display()))?;
        current = if target.is_absolute() {
            target
        } else {
            current
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(target)
        };
    }
    anyhow::bail!(
        "symlink chain exceeds safe depth while installing {}",
        path.display()
    )
}

fn normalize_absolute_path(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .context("resolve current directory for lifecycle installation")?
            .join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            std::path::Component::RootDir => normalized.push(component.as_os_str()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                anyhow::ensure!(
                    normalized.pop(),
                    "lifecycle path escapes the filesystem root: {}",
                    path.display()
                );
            }
            std::path::Component::Normal(segment) => normalized.push(segment),
        }
    }
    Ok(normalized)
}

fn canonicalize_with_missing_tail(path: &Path) -> Result<PathBuf> {
    let mut current = normalize_absolute_path(path)?;
    let mut missing = Vec::new();
    loop {
        match std::fs::canonicalize(&current) {
            Ok(mut resolved) => {
                for segment in missing.iter().rev() {
                    resolved.push(segment);
                }
                return Ok(resolved);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if std::fs::symlink_metadata(&current)
                    .is_ok_and(|metadata| metadata.file_type().is_symlink())
                {
                    return Err(error).with_context(|| {
                        format!("resolve lifecycle symlink {}", current.display())
                    });
                }
                let segment = current.file_name().ok_or_else(|| {
                    anyhow::anyhow!("no existing ancestor for lifecycle path {}", path.display())
                })?;
                missing.push(segment.to_os_string());
                anyhow::ensure!(
                    current.pop(),
                    "no existing ancestor for lifecycle path {}",
                    path.display()
                );
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("resolve lifecycle path {}", current.display()));
            }
        }
    }
}

pub(super) fn resolve_confined_write_destination(path: &Path, workspace: &Path) -> Result<PathBuf> {
    let write_destination = resolve_write_destination(path)?;
    let resolved_workspace = canonicalize_with_missing_tail(workspace)?;
    let resolved_destination = canonicalize_with_missing_tail(&write_destination)?;
    anyhow::ensure!(
        resolved_destination.starts_with(&resolved_workspace),
        "lifecycle artifact {} resolves outside workspace {}",
        path.display(),
        workspace.display()
    );
    Ok(write_destination)
}

pub(crate) fn write_workspace_artifact(
    root: &Path,
    relative: &Path,
    contents: &str,
    mode: u32,
) -> Result<()> {
    let destination = root.join(relative);
    let write_destination = resolve_confined_write_destination(&destination, root)?;
    write_static_file(&destination, &write_destination, contents, mode)
}
