//! Read a stable private SQLite image, including committed WAL frames.
use anyhow::{Context as _, Result, bail, ensure};
use rusqlite::Connection;
use std::{
    fs,
    io::{Read as _, Write as _},
    os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _},
    path::{Path, PathBuf},
};

const LIMIT: u64 = 64 * 1024 * 1024;
const ATTEMPTS: usize = 3;

#[cfg(test)]
mod tests;

struct PrivateImage(PathBuf);
impl Drop for PrivateImage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn read<T>(
    path: &Path,
    query: impl FnOnce(&Connection) -> Result<T>,
) -> Result<Option<T>> {
    read_with(path, query, || {})
}

fn read_with<T>(
    path: &Path,
    query: impl FnOnce(&Connection) -> Result<T>,
    mut captured: impl FnMut(),
) -> Result<Option<T>> {
    check_ancestors(path)?;
    let wal = PathBuf::from(format!("{}-wal", path.display()));
    for _ in 0..ATTEMPTS {
        check_ancestors(path)?;
        let before = (capture(path)?, capture(&wal)?);
        captured();
        if before != (capture(path)?, capture(&wal)?) {
            continue;
        }
        check_ancestors(path)?;
        let (database, wal) = before;
        let Some(database) = database else {
            ensure!(wal.is_none(), "SQLite snapshot WAL has no database");
            return Ok(None);
        };
        let directory =
            std::env::temp_dir().join(format!("brain-state-probe-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&directory)?;
        let private = PrivateImage(directory);
        let target = private.0.join("journal.db");
        write_private(&target, &database)?;
        if let Some(wal) = wal {
            write_private(&private.0.join("journal.db-wal"), &wal)?;
        }
        let connection =
            Connection::open_with_flags(&target, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        return query(&connection).map(Some);
    }
    bail!("SQLite snapshot changed during bounded snapshot capture")
}

fn capture(path: &Path) -> Result<Option<Vec<u8>>> {
    let file = match fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file(),
        "SQLite snapshot component is not a regular file"
    );
    ensure!(
        metadata.len() <= LIMIT,
        "SQLite snapshot component exceeds 64 MiB"
    );
    let mut bytes = Vec::new();
    file.take(LIMIT + 1).read_to_end(&mut bytes)?;
    ensure!(
        u64::try_from(bytes.len())? <= LIMIT,
        "SQLite snapshot component exceeds 64 MiB"
    );
    Ok(Some(bytes))
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?
        .write_all(bytes)?;
    Ok(())
}

fn check_ancestors(path: &Path) -> Result<()> {
    ensure!(path.is_absolute(), "SQLite snapshot path must be absolute");
    let mut current = PathBuf::from("/");
    let mut components = path
        .parent()
        .context("SQLite snapshot parent")?
        .components()
        .skip(1);
    if let Some(first) = components.next() {
        current.push(first);
        if current.exists() {
            current = current.canonicalize()?;
        }
    }
    for component in components {
        ensure!(
            matches!(component, std::path::Component::Normal(_)),
            "invalid SQLite snapshot ancestor"
        );
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "unsafe SQLite snapshot ancestor"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
