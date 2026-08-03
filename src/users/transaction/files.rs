//! Filesystem primitives for portable-user transaction artifacts.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::users::UsersError;

use super::PreparedChange;

pub(super) fn cleanup_orphans(root: &Path) -> Result<(), UsersError> {
    for directory in [root.join(".config"), root.join("tasks")] {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(io_error(
                    "inspect user transaction artifacts",
                    &directory,
                    &error,
                ));
            }
        };
        for entry in entries {
            let entry = entry.map_err(|error| {
                io_error("inspect user transaction artifact", &directory, &error)
            })?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if is_transaction_artifact(&name) {
                let path = entry.path();
                fs::remove_file(&path)
                    .map_err(|error| io_error("remove user transaction artifact", &path, &error))?;
            }
        }
    }
    Ok(())
}

fn is_transaction_artifact(name: &str) -> bool {
    name == ".brain-user-transaction.json.tmp"
        || (name.starts_with(".brain-user-")
            && (name.ends_with(".staged")
                || name.ends_with(".backup")
                || name.ends_with(".backup.restore")))
}

pub(super) fn cleanup_prepared(root: &Path, prepared: &[PreparedChange]) {
    for change in prepared {
        for path in [&change.staged, &change.backup] {
            let _ = fs::remove_file(root.join(path));
        }
    }
}

pub(super) fn write_new(path: &Path, bytes: &[u8], mode: u32) -> Result<(), UsersError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    let mut file = options
        .open(path)
        .map_err(|error| io_error("create user transaction file", path, &error))?;
    file.write_all(bytes)
        .map_err(|error| io_error("write user transaction file", path, &error))?;
    file.sync_all()
        .map_err(|error| io_error("sync user transaction file", path, &error))?;
    Ok(())
}

pub(super) fn file_mode(path: &Path) -> Result<u32, UsersError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .map_err(|error| io_error("read user transaction permissions", path, &error))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(0o600)
    }
}

pub(super) fn relative_path(root: &Path, path: &Path) -> Result<PathBuf, UsersError> {
    let relative = path.strip_prefix(root).map_err(|_| {
        transaction_error(format!("{} is outside {}", path.display(), root.display()))
    })?;
    validate_relative(relative)?;
    Ok(relative.to_path_buf())
}

pub(super) fn validate_relative(path: &Path) -> Result<(), UsersError> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return Err(transaction_error(format!(
            "unsafe transaction path {}",
            path.display()
        )));
    }
    Ok(())
}

pub(super) fn sibling_path(
    path: &Path,
    nonce: &str,
    index: usize,
    suffix: &str,
) -> Result<PathBuf, UsersError> {
    let parent = path
        .parent()
        .ok_or_else(|| transaction_error(format!("{} has no parent directory", path.display())))?;
    Ok(parent.join(format!(".brain-user-{nonce}-{index}.{suffix}")))
}

pub(super) fn restore_path(backup: &Path) -> PathBuf {
    let name = backup
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(".brain-user-backup");
    backup.with_file_name(format!("{name}.restore"))
}

pub(super) fn transaction_nonce() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("{}-{nanos}", std::process::id())
}

pub(super) fn sync_parent(path: &Path) -> Result<(), UsersError> {
    let parent = path
        .parent()
        .ok_or_else(|| transaction_error(format!("{} has no parent directory", path.display())))?;
    let directory = fs::File::open(parent)
        .map_err(|error| io_error("open user transaction parent", parent, &error))?;
    directory
        .sync_all()
        .map_err(|error| io_error("sync user transaction parent", parent, &error))
}

pub(super) fn io_error(operation: &str, path: &Path, error: &std::io::Error) -> UsersError {
    transaction_error(format!("{operation} at {}: {error}", path.display()))
}

pub(super) fn transaction_error(message: String) -> UsersError {
    UsersError::Transaction { message }
}
