use std::fs;
use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

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
    if legacy_endpoint_may_be_live(path) {
        return Ok(());
    }
    let parent = fs::canonicalize(
        path.parent()
            .context("resolve legacy receiver endpoint parent")?,
    )
    .context("resolve legacy receiver endpoint owner")?;
    crate::workspace::remove_socket_file_beneath(
        &parent,
        Path::new("jobs.sock"),
        observed.device,
        observed.inode,
        observed.uid,
    )
    .context("unlink stale legacy receiver endpoint")
}

fn legacy_endpoint_may_be_live(path: &Path) -> bool {
    if legacy_tui_is_live(path) {
        return true;
    }
    let deadline = Instant::now() + Duration::from_millis(25);
    match crate::server::control::connect::connect_until(path, deadline) {
        Ok(_) => true,
        Err(error) => !error.chain().any(|cause| {
            cause
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::ConnectionRefused)
        }),
    }
}

fn legacy_tui_is_live(path: &Path) -> bool {
    path.parent()
        .and_then(|parent| fs::read_to_string(parent.join("tui.lock")).ok())
        .and_then(|pid| pid.trim().parse::<u32>().ok())
        .is_some_and(crate::server::lifecycle::pid_alive)
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct SocketIdentity {
    device: u64,
    inode: u64,
    uid: u32,
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
        uid: metadata.uid(),
    }))
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{FileTypeExt as _, PermissionsExt as _};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::Path;
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::{Duration, Instant};

    use super::remove_stale_owned_socket;
    use crate::workspace::{SecureRemoveTestBoundary, with_secure_remove_test_hook};

    fn bind_stale(path: &Path) {
        std::fs::create_dir_all(path.parent().expect("socket parent")).expect("socket parent");
        drop(UnixListener::bind(path).expect("stale socket"));
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .expect("owner-only socket");
    }

    fn socket_leaves_beneath(path: &Path) -> usize {
        let Ok(entries) = std::fs::read_dir(path) else {
            return 0;
        };
        entries
            .flatten()
            .map(|entry| {
                let path = entry.path();
                let is_socket = std::fs::symlink_metadata(&path)
                    .is_ok_and(|metadata| metadata.file_type().is_socket());
                usize::from(is_socket) + socket_leaves_beneath(&path)
            })
            .sum()
    }

    #[test]
    fn replacement_after_identity_check_is_never_unlinked() {
        let temporary = tempfile::tempdir().expect("temporary home");
        let home = temporary.path().canonicalize().expect("canonical home");
        let parent = home.join("cache");
        let path = parent.join("jobs.sock");
        bind_stale(&path);
        let replacement = Arc::new(Mutex::new(None));
        let held = Arc::clone(&replacement);
        let replaced_path = path.clone();

        let result = with_secure_remove_test_hook(
            move |boundary, _| {
                if boundary == SecureRemoveTestBoundary::EntryIdentityVerifiedBeforeRename {
                    std::fs::remove_file(&replaced_path).expect("retire observed socket");
                    let listener = UnixListener::bind(&replaced_path).expect("replacement socket");
                    std::fs::set_permissions(
                        &replaced_path,
                        std::fs::Permissions::from_mode(0o600),
                    )
                    .expect("owner-only replacement");
                    *held.lock().expect("replacement owner") = Some(listener);
                }
            },
            || remove_stale_owned_socket(&path, &home),
        );

        assert!(result.is_err(), "replacement race must fail closed");
        assert_eq!(
            socket_leaves_beneath(&parent),
            1,
            "replacement socket was unlinked"
        );
        assert!(replacement.lock().expect("replacement owner").is_some());
    }

    #[test]
    fn a_full_live_listener_never_blocks_startup_probe() {
        let temporary = tempfile::tempdir().expect("temporary home");
        let home = temporary.path().to_path_buf();
        let path = home.join("cache/jobs.sock");
        std::fs::create_dir_all(path.parent().expect("socket parent")).expect("socket parent");
        let listener = UnixListener::bind(&path).expect("live listener");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .expect("owner-only socket");
        std::fs::write(
            path.parent().expect("socket parent").join("tui.lock"),
            std::process::id().to_string(),
        )
        .expect("live legacy singleton");
        let mut pending = Vec::<UnixStream>::new();
        loop {
            let deadline = Instant::now() + Duration::from_millis(20);
            match crate::server::control::connect::connect_until(&path, deadline) {
                Ok(stream) => pending.push(stream),
                Err(_) => break,
            }
            assert!(pending.len() < 1_024, "listener backlog never saturated");
        }
        let worker_path = path.clone();
        let (finished_tx, finished_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let result = remove_stale_owned_socket(&worker_path, &home);
            finished_tx.send(result).expect("publish probe result");
        });

        let bounded = finished_rx.recv_timeout(Duration::from_millis(250));
        drop(pending);
        drop(listener);
        let _ = worker.join();

        assert!(
            bounded.is_ok(),
            "legacy socket probe exceeded its bounded budget"
        );
        assert!(path.exists(), "an uncertain live listener was removed");
    }
}
