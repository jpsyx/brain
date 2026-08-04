//! Symlink-safe operations below one workspace's trusted runtime-cache root.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path};

use super::CapabilityError;

pub(crate) fn ensure_directory(
    workspace: &crate::workspace::WorkspaceContext,
    path: &Path,
) -> Result<(), CapabilityError> {
    let root = workspace.paths().cache_dir();
    let relative = checked_relative(root, path)?;
    ensure_cache_root(root)?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err(invalid_path(path));
        };
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(not_real_directory(&current));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|error| runtime_error(&error))?;
            }
            Err(error) => return Err(runtime_error(&error)),
        }
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|error| runtime_error(&error))
}

pub(crate) fn existing_directory(
    workspace: &crate::workspace::WorkspaceContext,
    path: &Path,
) -> Result<bool, CapabilityError> {
    let Some(metadata) = metadata_without_following(workspace, path)? else {
        return Ok(false);
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(not_real_directory(path));
    }
    Ok(true)
}

pub(crate) fn remove_path(
    workspace: &crate::workspace::WorkspaceContext,
    path: &Path,
) -> Result<bool, CapabilityError> {
    let Some(metadata) = metadata_without_following(workspace, path)? else {
        return Ok(false);
    };
    if metadata.file_type().is_symlink() || metadata.is_file() {
        fs::remove_file(path).map_err(|error| runtime_error(&error))?;
    } else if metadata.is_dir() {
        fs::remove_dir_all(path).map_err(|error| runtime_error(&error))?;
    } else {
        return Err(CapabilityError::RuntimeArtifact(format!(
            "unsupported capability artifact at {}",
            path.display()
        )));
    }
    Ok(true)
}

fn ensure_cache_root(root: &Path) -> Result<(), CapabilityError> {
    match fs::symlink_metadata(root) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(not_real_directory(root))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(root).map_err(|error| runtime_error(&error))?;
            let metadata = fs::symlink_metadata(root).map_err(|error| runtime_error(&error))?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(not_real_directory(root));
            }
            Ok(())
        }
        Err(error) => Err(runtime_error(&error)),
    }
}

fn metadata_without_following(
    workspace: &crate::workspace::WorkspaceContext,
    path: &Path,
) -> Result<Option<fs::Metadata>, CapabilityError> {
    let root = workspace.paths().cache_dir();
    let relative = checked_relative(root, path)?;
    let root_metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(runtime_error(&error)),
    };
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(not_real_directory(root));
    }
    let components = relative.components().collect::<Vec<_>>();
    let mut current = root.to_path_buf();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(component) = component else {
            return Err(invalid_path(path));
        };
        current.push(component);
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(runtime_error(&error)),
        };
        if index + 1 == components.len() {
            return Ok(Some(metadata));
        }
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(not_real_directory(&current));
        }
    }
    Ok(Some(root_metadata))
}

fn checked_relative<'a>(root: &Path, path: &'a Path) -> Result<&'a Path, CapabilityError> {
    let relative = path.strip_prefix(root).map_err(|_| invalid_path(path))?;
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(invalid_path(path));
    }
    Ok(relative)
}

fn not_real_directory(path: &Path) -> CapabilityError {
    CapabilityError::RuntimeArtifact(format!(
        "{} must be a real directory; symlinked capability-cache ancestors are rejected",
        path.display()
    ))
}

fn invalid_path(path: &Path) -> CapabilityError {
    CapabilityError::RuntimeArtifact(format!(
        "{} is outside the trusted workspace cache root",
        path.display()
    ))
}

fn runtime_error(error: &std::io::Error) -> CapabilityError {
    CapabilityError::RuntimeArtifact(error.to_string())
}
