//! Path normalization and disjoint backup validation.

use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};

pub(super) fn validate_backup_destination(
    workspace_root: &Path,
    preexisting_backup_base: &Path,
    backup_dir: &Path,
) -> Result<(PathBuf, PathBuf, PathBuf)> {
    let workspace_root = resolve_path(workspace_root)?;
    let backup_base = fs::canonicalize(preexisting_backup_base).with_context(|| {
        format!(
            "task migration backup base must already exist and be durable: {}",
            preexisting_backup_base.display()
        )
    })?;
    if !backup_base.is_dir() {
        bail!(
            "task migration backup base must already exist as a directory: {}",
            preexisting_backup_base.display()
        );
    }
    let backup_dir = resolve_path(backup_dir)?;
    if workspace_root.starts_with(&backup_dir) || backup_dir.starts_with(&workspace_root) {
        bail!(
            "task migration backup must be disjoint from the workspace tree: {} and {}",
            backup_dir.display(),
            workspace_root.display()
        );
    }
    if !backup_dir.starts_with(&backup_base) {
        bail!(
            "task migration backup must be at or below its pre-existing durable base: {} and {}",
            backup_dir.display(),
            backup_base.display()
        );
    }
    Ok((workspace_root, backup_base, backup_dir))
}

fn resolve_path(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() {
        bail!("task migration paths must be absolute: {}", path.display());
    }
    let normalized = normalize_absolute(path)?;
    let mut existing = normalized.as_path();
    let mut missing = Vec::new();
    let canonical = loop {
        match fs::canonicalize(existing) {
            Ok(canonical) => break canonical,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let name = existing.file_name().ok_or_else(|| {
                    anyhow!("cannot resolve task migration path {}", path.display())
                })?;
                missing.push(name.to_owned());
                existing = existing.parent().ok_or_else(|| {
                    anyhow!("cannot resolve task migration path {}", path.display())
                })?;
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("resolving task migration path {}", path.display()));
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
                        "task migration path escapes its filesystem root: {}",
                        path.display()
                    );
                }
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    if !normalized.is_absolute() {
        bail!("task migration paths must be absolute: {}", path.display());
    }
    Ok(normalized)
}
