use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};

fn hook_lock_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("hooks.json");
    path.with_file_name(format!(".{file_name}.transaction.lock"))
}

pub(super) fn hook_temporary_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("hooks.json");
    path.with_file_name(format!(".{file_name}.tmp-{}", uuid::Uuid::new_v4()))
}

pub(crate) fn update_json_file(
    path: &Path,
    mutation: impl FnOnce(&mut serde_json::Value),
) -> Result<()> {
    update_json_file_with_temporary_and_lock(path, &hook_temporary_path(path), mutation, || Ok(()))
}

#[cfg(test)]
pub(super) fn update_json_file_with_temporary(
    path: &Path,
    temporary: &Path,
    mutation: impl FnOnce(&mut serde_json::Value),
) -> Result<()> {
    update_json_file_with_temporary_and_lock(path, temporary, mutation, || Ok(()))
}

pub(super) fn update_json_file_with_temporary_and_lock(
    path: &Path,
    temporary: &Path,
    mutation: impl FnOnce(&mut serde_json::Value),
    after_lock_created: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .with_context(|| format!("create hook settings directory {}", parent.display()))?;
    let lock_path = hook_lock_path(path);
    let lock_existed = lock_path.exists();
    let result = (|| {
        let connection = rusqlite::Connection::open(&lock_path)
            .with_context(|| format!("open hook settings lock {}", lock_path.display()))?;
        after_lock_created()?;
        connection
            .busy_timeout(Duration::from_secs(10))
            .context("configure hook settings lock")?;
        connection
            .execute_batch("PRAGMA journal_mode = OFF; BEGIN IMMEDIATE")
            .with_context(|| format!("acquire hook settings lock {}", lock_path.display()))?;

        let existing = match std::fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("read hook settings {}", path.display()));
            }
        };
        let mut settings = match existing.as_deref() {
            Some(bytes) => serde_json::from_slice(bytes)
                .with_context(|| format!("parse hook settings {}", path.display()))?,
            None => serde_json::json!({}),
        };
        mutation(&mut settings);
        let mut bytes = serde_json::to_vec_pretty(&settings)
            .with_context(|| format!("serialize hook settings {}", path.display()))?;
        bytes.push(b'\n');
        // Reinstallation is idempotent on disk: settings that already say what
        // this Brain would write keep their mtime, so the workspace watcher has
        // nothing to push. See `needs_rewrite`.
        if existing.as_deref() == Some(bytes.as_slice()) {
            return Ok(());
        }

        write_and_replace_json(temporary, path, &bytes)
    })();
    if result.is_err() && !lock_existed {
        let _ = std::fs::remove_file(lock_path);
    }
    result
}

fn write_and_replace_json(temporary: &Path, destination: &Path, bytes: &[u8]) -> Result<()> {
    let write_destination = match std::fs::symlink_metadata(destination) {
        Ok(metadata) if metadata.file_type().is_symlink() => std::fs::canonicalize(destination)
            .with_context(|| format!("resolve hook settings target {}", destination.display()))?,
        Ok(_) => destination.to_path_buf(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => destination.to_path_buf(),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("inspect hook settings {}", destination.display()));
        }
    };
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(temporary)
        .with_context(|| format!("create temporary hook settings {}", temporary.display()))?;
    let result = (|| {
        file.write_all(bytes)
            .with_context(|| format!("write temporary hook settings {}", temporary.display()))?;
        file.sync_all()
            .with_context(|| format!("sync temporary hook settings {}", temporary.display()))?;
        drop(file);
        std::fs::rename(temporary, &write_destination).with_context(|| {
            format!(
                "replace hook settings {} from {}",
                destination.display(),
                temporary.display()
            )
        })?;
        if let Some(parent) = write_destination.parent()
            && let Ok(directory) = std::fs::File::open(parent)
        {
            let _ = directory.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}
