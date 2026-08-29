use std::fs;
use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _, PermissionsExt as _};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

pub(super) fn up(home: &Path) -> Result<()> {
    for path in legacy_socket_paths(home) {
        remove_stale_owned_socket(&path, home).context("remove legacy receiver endpoint")?;
    }
    Ok(())
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "migration callbacks share one fallible up/down signature"
)]
pub(super) const fn down(_home: &Path) -> Result<()> {
    Ok(())
}

fn legacy_socket_paths(home: &Path) -> Vec<PathBuf> {
    let store = crate::workspace::RegistryStore::real();
    let Ok(registry) = crate::workspace::RegistryStore::load_readable(store.path()) else {
        return Vec::new();
    };
    registry
        .workspaces
        .values()
        .map(|record| {
            crate::workspace::WorkspacePaths::new(home, record.workspace_id)
                .cache_dir()
                .join("jobs.sock")
        })
        .collect()
}

fn remove_stale_owned_socket(path: &Path, home: &Path) -> Result<()> {
    let Some(observed) = socket_identity(path, home)? else {
        return Ok(());
    };
    match UnixStream::connect(path) {
        Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => {}
        Ok(_) | Err(_) => return Ok(()),
    }
    if socket_identity(path, home)? != Some(observed) {
        return Ok(());
    }
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).context("unlink stale legacy receiver endpoint"),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct SocketIdentity {
    device: u64,
    inode: u64,
}

fn socket_identity(path: &Path, home: &Path) -> Result<Option<SocketIdentity>> {
    let Some(parent) = path.parent() else {
        return Ok(None);
    };
    let parent_metadata = match fs::symlink_metadata(parent) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("inspect legacy receiver endpoint owner"),
    };
    let home_uid = fs::metadata(home).context("inspect migration owner")?.uid();
    if !parent_metadata.file_type().is_dir()
        || parent_metadata.uid() != home_uid
        || parent_metadata.permissions().mode() & 0o022 != 0
    {
        return Ok(None);
    }
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("inspect legacy receiver endpoint"),
    };
    if !metadata.file_type().is_socket()
        || metadata.uid() != home_uid
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Ok(None);
    }
    Ok(Some(SocketIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }))
}
