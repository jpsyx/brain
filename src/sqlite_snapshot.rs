//! Read a stable private SQLite image, including committed WAL frames.
use anyhow::{Context as _, Result, bail, ensure};
use rusqlite::Connection;
use sha2::{Digest as _, Sha256};
use std::{
    fs,
    io::{Read as _, Write as _},
    os::unix::fs::{DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _},
    path::{Path, PathBuf},
};

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
        let directory =
            std::env::temp_dir().join(format!("brain-state-probe-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&directory)?;
        let private = PrivateImage(directory);
        let target = private.0.join("journal.db");
        let before = (
            capture(path, Some(&target))?,
            capture(&wal, Some(&private.0.join("journal.db-wal")))?,
        );
        captured();
        if before != (capture(path, None)?, capture(&wal, None)?) {
            continue;
        }
        check_ancestors(path)?;
        let (database, wal) = before;
        if database.is_none() {
            ensure!(wal.is_none(), "SQLite snapshot WAL has no database");
            return Ok(None);
        }
        let connection =
            Connection::open_with_flags(&target, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        return query(&connection).map(Some);
    }
    bail!("SQLite snapshot changed during bounded snapshot capture")
}

#[derive(PartialEq, Eq)]
struct Fingerprint {
    identity: [u64; 3],
    times: [i64; 4],
    digest: [u8; 32],
}

fn capture(path: &Path, target: Option<&Path>) -> Result<Option<Fingerprint>> {
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
    let mut output = target
        .map(|path| {
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(path)
        })
        .transpose()?;
    let mut reader = file.take(metadata.len().saturating_add(1));
    let mut digest = Sha256::new();
    let mut buffer = [0; 8192];
    let mut length = 0_u64;
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        length += u64::try_from(count)?;
        digest.update(&buffer[..count]);
        if let Some(output) = &mut output {
            output.write_all(&buffer[..count])?;
        }
    }
    ensure!(
        length == metadata.len(),
        "SQLite snapshot component changed size during capture"
    );
    Ok(Some(Fingerprint {
        identity: [metadata.dev(), metadata.ino(), length],
        times: [
            metadata.mtime(),
            metadata.mtime_nsec(),
            metadata.ctime(),
            metadata.ctime_nsec(),
        ],
        digest: digest.finalize().into(),
    }))
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
