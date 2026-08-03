//! Portable users path resolution and same-directory atomic persistence.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{Users, UsersError};
use crate::workspace::WorkspaceContext;

static TEMPORARY_COUNTER: AtomicU64 = AtomicU64::new(0);

/// The portable user registry store.
pub struct UsersStore;

impl UsersStore {
    /// Resolve `<workspace-root>/.config/users.json`.
    #[must_use]
    pub fn path(workspace: &WorkspaceContext) -> PathBuf {
        workspace.root().join(".config/users.json")
    }

    /// Load and validate the selected workspace's portable users.
    pub fn load(workspace: &WorkspaceContext) -> Result<Users, UsersError> {
        Self::load_from(&Self::path(workspace))
    }

    /// Load and validate portable users from an injected path.
    pub fn load_from(path: &Path) -> Result<Users, UsersError> {
        let bytes =
            fs::read(path).map_err(|error| io_error("read portable users", path, None, &error))?;
        Users::parse(&bytes)
    }

    /// Atomically replace the selected workspace's portable users.
    pub fn save(workspace: &WorkspaceContext, users: &Users) -> Result<(), UsersError> {
        Self::save_to(&Self::path(workspace), users)
    }

    /// Atomically replace an injected portable-users path.
    pub fn save_to(path: &Path, users: &Users) -> Result<(), UsersError> {
        let bytes = users.to_bytes()?;
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(|error| {
            io_error(
                "create portable users directory",
                parent,
                Some(path),
                &error,
            )
        })?;
        let temporary = temporary_path(path);
        let result = write_and_replace(&temporary, path, &bytes);
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

fn temporary_path(path: &Path) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let counter = TEMPORARY_COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("users.json");
    path.with_file_name(format!(
        ".{name}.tmp-{}-{nonce}-{counter}",
        std::process::id()
    ))
}

fn write_and_replace(temporary: &Path, path: &Path, bytes: &[u8]) -> Result<(), UsersError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(temporary).map_err(|error| {
        io_error(
            "create temporary portable users",
            temporary,
            Some(path),
            &error,
        )
    })?;
    file.write_all(bytes).map_err(|error| {
        io_error(
            "write temporary portable users",
            temporary,
            Some(path),
            &error,
        )
    })?;
    file.sync_all().map_err(|error| {
        io_error(
            "sync temporary portable users",
            temporary,
            Some(path),
            &error,
        )
    })?;
    drop(file);
    fs::rename(temporary, path)
        .map_err(|error| io_error("replace portable users", path, Some(temporary), &error))?;
    if let Some(parent) = path.parent()
        && let Ok(directory) = fs::File::open(parent)
    {
        let _ = directory.sync_all();
    }
    Ok(())
}

fn io_error(
    operation: &'static str,
    path: &Path,
    related_path: Option<&Path>,
    error: &std::io::Error,
) -> UsersError {
    UsersError::Io {
        operation,
        path: path.to_path_buf(),
        related_path: related_path.map(Path::to_path_buf),
        kind: error.kind(),
        message: error.to_string(),
    }
}
