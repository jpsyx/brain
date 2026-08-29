use std::fs;
use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

const MAX_LEGACY_SINGLETON_BYTES: usize = 32;

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
    let Some((parent, owner_uid)) = verified_socket_parent(path, home)? else {
        return Ok(());
    };
    let recovered =
        crate::workspace::recover_socket_file_beneath(&parent, Path::new("jobs.sock"), owner_uid)
            .context("recover legacy receiver endpoint")?;
    if recovered {
        return Ok(());
    }
    let Some(observed) = socket_identity(path, home)? else {
        return Ok(());
    };
    #[cfg(test)]
    crate::workspace::observe_secure_remove_test_boundary(
        crate::workspace::SecureRemoveTestBoundary::LegacySocketIdentityObservedBeforeLiveness,
        Path::new("jobs.sock"),
    );
    if legacy_endpoint_may_be_live(&parent, observed.uid) {
        return Ok(());
    }
    crate::workspace::remove_socket_file_beneath(
        &parent,
        Path::new("jobs.sock"),
        observed.device,
        observed.inode,
        observed.uid,
    )
    .context("unlink stale legacy receiver endpoint")
}

fn verified_socket_parent(path: &Path, home: &Path) -> Result<Option<(PathBuf, u32)>> {
    let Some(parent) = path.parent() else {
        return Ok(None);
    };
    let metadata = match fs::symlink_metadata(parent) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("inspect legacy receiver endpoint owner"),
    };
    let owner_uid = fs::metadata(home).context("inspect migration owner")?.uid();
    if !metadata.file_type().is_dir()
        || metadata.uid() != owner_uid
        || metadata.permissions().mode() & 0o022 != 0
    {
        return Ok(None);
    }
    let parent = fs::canonicalize(parent).context("resolve legacy receiver endpoint owner")?;
    Ok(Some((parent, owner_uid)))
}

fn legacy_endpoint_may_be_live(parent: &Path, expected_uid: u32) -> bool {
    !matches!(
        legacy_tui_status(parent, expected_uid),
        LegacyTuiStatus::Inactive
    )
}

fn legacy_tui_status(parent: &Path, expected_uid: u32) -> LegacyTuiStatus {
    let singleton = crate::workspace::read_small_owned_regular_file_beneath(
        parent,
        Path::new("tui.lock"),
        expected_uid,
        MAX_LEGACY_SINGLETON_BYTES,
    );
    let contents = match singleton {
        Ok(Some(contents)) => contents,
        Ok(None) => return LegacyTuiStatus::Inactive,
        Err(_) => return LegacyTuiStatus::Untrusted,
    };
    let Some(pid) = std::str::from_utf8(&contents)
        .ok()
        .and_then(|contents| contents.trim().parse::<u32>().ok())
    else {
        return LegacyTuiStatus::Untrusted;
    };
    if pid > 0 && crate::server::lifecycle::pid_alive(pid) {
        LegacyTuiStatus::Live
    } else {
        LegacyTuiStatus::Inactive
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LegacyTuiStatus {
    Live,
    Inactive,
    Untrusted,
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
    use std::io::Write as _;
    use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _, PermissionsExt as _};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::Path;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
        mpsc,
    };
    use std::time::{Duration, Instant};

    use nix::sys::stat::Mode;
    use nix::unistd::mkfifo;

    use super::remove_stale_owned_socket;
    use crate::workspace::{SecureRemoveTestBoundary, with_secure_remove_test_hook};

    fn bind_stale(path: &Path) {
        std::fs::create_dir_all(path.parent().expect("socket parent")).expect("socket parent");
        drop(UnixListener::bind(path).expect("stale socket"));
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .expect("owner-only socket");
    }

    fn replace_with_listener(path: &Path) -> (UnixListener, u64) {
        let listener = UnixListener::bind(path).expect("replacement socket");
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .expect("owner-only replacement");
        let inode = std::fs::symlink_metadata(path)
            .expect("replacement metadata")
            .ino();
        (listener, inode)
    }

    #[test]
    fn immediate_restore_retains_authority_when_a_later_leaf_wins_the_race() {
        let temporary = tempfile::tempdir().expect("temporary home");
        let home = temporary.path().canonicalize().expect("canonical home");
        let path = home.join("cache/jobs.sock");
        bind_stale(&path);
        let replacement = Arc::new(Mutex::new(None));
        let later = Arc::new(Mutex::new(None));
        let replacement_inode = Arc::new(AtomicU64::new(0));
        let later_inode = Arc::new(AtomicU64::new(0));
        let held_replacement = Arc::clone(&replacement);
        let held_later = Arc::clone(&later);
        let recorded_replacement_inode = Arc::clone(&replacement_inode);
        let recorded_later_inode = Arc::clone(&later_inode);
        let raced_path = path.clone();

        let result = with_secure_remove_test_hook(
            move |boundary, _| match boundary {
                SecureRemoveTestBoundary::EntryIdentityVerifiedBeforeRename => {
                    std::fs::remove_file(&raced_path).expect("retire observed socket");
                    let (listener, inode) = replace_with_listener(&raced_path);
                    recorded_replacement_inode.store(inode, Ordering::SeqCst);
                    *held_replacement.lock().expect("replacement owner") = Some(listener);
                }
                SecureRemoveTestBoundary::SocketRestoredBeforeAuthorityRetention => {
                    std::fs::remove_file(&raced_path).expect("race restored socket");
                    let (listener, inode) = replace_with_listener(&raced_path);
                    recorded_later_inode.store(inode, Ordering::SeqCst);
                    *held_later.lock().expect("later owner") = Some(listener);
                }
                _ => {}
            },
            || remove_stale_owned_socket(&path, &home),
        );

        assert!(result.is_err(), "replacement race must fail closed");
        assert_eq!(
            std::fs::symlink_metadata(&path)
                .expect("later socket metadata")
                .ino(),
            later_inode.load(Ordering::SeqCst),
            "the later leaf did not remain at jobs.sock"
        );
        drop(later.lock().expect("later owner").take());
        std::fs::remove_file(&path).expect("retire later leaf");

        let restarted = remove_stale_owned_socket(&path, &home);

        assert!(restarted.is_ok(), "restart recovery failed: {restarted:?}");
        assert_eq!(
            std::fs::symlink_metadata(&path)
                .expect("recovered socket metadata")
                .ino(),
            replacement_inode.load(Ordering::SeqCst),
            "restart did not restore the quarantined socket authority"
        );
    }

    #[test]
    fn restart_restore_retains_authority_when_a_later_leaf_wins_the_race() {
        let temporary = tempfile::tempdir().expect("temporary home");
        let home = temporary.path().canonicalize().expect("canonical home");
        let path = home.join("cache/jobs.sock");
        bind_stale(&path);
        let replacement = Arc::new(Mutex::new(None));
        let replacement_inode = Arc::new(AtomicU64::new(0));
        let held_replacement = Arc::clone(&replacement);
        let recorded_replacement_inode = Arc::clone(&replacement_inode);
        let raced_path = path.clone();

        let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            with_secure_remove_test_hook(
                move |boundary, _| match boundary {
                    SecureRemoveTestBoundary::EntryIdentityVerifiedBeforeRename => {
                        std::fs::remove_file(&raced_path).expect("retire observed socket");
                        let (listener, inode) = replace_with_listener(&raced_path);
                        recorded_replacement_inode.store(inode, Ordering::SeqCst);
                        *held_replacement.lock().expect("replacement owner") = Some(listener);
                    }
                    SecureRemoveTestBoundary::QuarantineRenameBeforeVerification => {
                        panic!("simulated process interruption after quarantine rename");
                    }
                    _ => {}
                },
                || remove_stale_owned_socket(&path, &home),
            )
        }));
        assert!(interrupted.is_err(), "test did not interrupt quarantine");

        let later = Arc::new(Mutex::new(None));
        let later_inode = Arc::new(AtomicU64::new(0));
        let held_later = Arc::clone(&later);
        let recorded_later_inode = Arc::clone(&later_inode);
        let raced_path = path.clone();
        let restarted = with_secure_remove_test_hook(
            move |boundary, _| {
                if boundary == SecureRemoveTestBoundary::SocketRestoredBeforeAuthorityRetention {
                    std::fs::remove_file(&raced_path).expect("race restored socket");
                    let (listener, inode) = replace_with_listener(&raced_path);
                    recorded_later_inode.store(inode, Ordering::SeqCst);
                    *held_later.lock().expect("later owner") = Some(listener);
                }
            },
            || remove_stale_owned_socket(&path, &home),
        );

        assert!(
            restarted.is_err(),
            "post-link replacement must fail closed: {restarted:?}"
        );
        assert_eq!(
            std::fs::symlink_metadata(&path)
                .expect("later socket metadata")
                .ino(),
            later_inode.load(Ordering::SeqCst),
            "the later leaf did not remain at jobs.sock"
        );
        drop(later.lock().expect("later owner").take());
        std::fs::remove_file(&path).expect("retire later leaf");

        let recovered = remove_stale_owned_socket(&path, &home);

        assert!(recovered.is_ok(), "second restart failed: {recovered:?}");
        assert_eq!(
            std::fs::symlink_metadata(&path)
                .expect("recovered socket metadata")
                .ino(),
            replacement_inode.load(Ordering::SeqCst),
            "restart recovery discarded the quarantined socket authority"
        );
    }

    #[test]
    fn socket_quarantine_discovery_has_a_total_directory_entry_budget() {
        const DIRECTORY_ENTRY_VISIT_LIMIT: usize = 256;

        let temporary = tempfile::tempdir().expect("temporary home");
        let home = temporary.path().canonicalize().expect("canonical home");
        let parent = home.join("cache");
        let path = parent.join("jobs.sock");
        bind_stale(&path);
        let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            with_secure_remove_test_hook(
                |boundary, _| {
                    assert_ne!(
                        boundary,
                        SecureRemoveTestBoundary::QuarantineRenameBeforeVerification,
                        "simulated process interruption after quarantine rename"
                    );
                },
                || remove_stale_owned_socket(&path, &home),
            )
        }));
        assert!(interrupted.is_err(), "test did not interrupt quarantine");
        let unrelated = (0..DIRECTORY_ENTRY_VISIT_LIMIT + 32)
            .map(|index| parent.join(format!("unrelated-{index:03}")))
            .collect::<Vec<_>>();
        for entry in &unrelated {
            std::fs::write(entry, b"unrelated").expect("unrelated cache entry");
        }
        let visited = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&visited);

        let bounded = with_secure_remove_test_hook(
            move |boundary, _| {
                if boundary == SecureRemoveTestBoundary::QuarantineDirectoryEntryVisited {
                    observed.fetch_add(1, Ordering::SeqCst);
                }
            },
            || remove_stale_owned_socket(&path, &home),
        );

        assert!(bounded.is_err(), "directory pressure must fail closed");
        assert!(
            visited.load(Ordering::SeqCst) <= DIRECTORY_ENTRY_VISIT_LIMIT,
            "quarantine discovery exceeded its total entry budget"
        );
        assert!(!path.exists(), "bounded scan mutated the exact socket leaf");
        for entry in unrelated {
            std::fs::remove_file(entry).expect("remove unrelated cache entry");
        }

        let recovered = remove_stale_owned_socket(&path, &home);

        assert!(
            recovered.is_ok(),
            "recovery after pressure failed: {recovered:?}"
        );
        assert!(
            std::fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_socket()),
            "restart did not recover the quarantined socket after pressure cleared"
        );
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
        assert!(
            std::fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_socket()),
            "replacement socket did not remain at jobs.sock"
        );
        assert!(replacement.lock().expect("replacement owner").is_some());
    }

    #[test]
    fn restart_restores_an_interrupted_replacement_to_jobs_sock() {
        let temporary = tempfile::tempdir().expect("temporary home");
        let home = temporary.path().canonicalize().expect("canonical home");
        let path = home.join("cache/jobs.sock");
        bind_stale(&path);
        let replacement = Arc::new(Mutex::new(None));
        let held = Arc::clone(&replacement);
        let replaced_path = path.clone();

        let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            with_secure_remove_test_hook(
                move |boundary, _| match boundary {
                    SecureRemoveTestBoundary::EntryIdentityVerifiedBeforeRename => {
                        std::fs::remove_file(&replaced_path).expect("retire observed socket");
                        let listener =
                            UnixListener::bind(&replaced_path).expect("replacement socket");
                        std::fs::set_permissions(
                            &replaced_path,
                            std::fs::Permissions::from_mode(0o600),
                        )
                        .expect("owner-only replacement");
                        *held.lock().expect("replacement owner") = Some(listener);
                    }
                    SecureRemoveTestBoundary::QuarantineRenameBeforeVerification => {
                        panic!("simulated process interruption after quarantine rename");
                    }
                    _ => {}
                },
                || remove_stale_owned_socket(&path, &home),
            )
        }));
        assert!(interrupted.is_err(), "test did not interrupt quarantine");
        assert!(
            std::fs::symlink_metadata(&path).is_err(),
            "interruption did not expose the missing-path recovery case"
        );

        let restarted = remove_stale_owned_socket(&path, &home);

        assert!(restarted.is_ok(), "restart recovery failed: {restarted:?}");
        assert!(
            std::fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_socket()),
            "restart did not restore the replacement to jobs.sock"
        );
        assert!(replacement.lock().expect("replacement owner").is_some());
    }

    #[test]
    fn restart_recovers_an_interrupted_empty_socket_quarantine() {
        let temporary = tempfile::tempdir().expect("temporary home");
        let home = temporary.path().canonicalize().expect("canonical home");
        let path = home.join("cache/jobs.sock");
        bind_stale(&path);

        let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            with_secure_remove_test_hook(
                |boundary, _| {
                    assert_ne!(
                        boundary,
                        SecureRemoveTestBoundary::QuarantineCreatedBeforeOpen,
                        "simulated process interruption after quarantine creation"
                    );
                },
                || remove_stale_owned_socket(&path, &home),
            )
        }));
        assert!(interrupted.is_err(), "test did not interrupt quarantine");
        assert!(
            path.exists(),
            "interruption unexpectedly removed the stale socket"
        );

        let restarted = remove_stale_owned_socket(&path, &home);

        assert!(restarted.is_ok(), "restart recovery failed: {restarted:?}");
        assert!(
            !path.exists(),
            "restart did not resume stale socket removal"
        );
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

    #[test]
    fn a_fifo_singleton_never_blocks_or_authorizes_socket_removal() {
        let temporary = tempfile::tempdir().expect("temporary home");
        let home = temporary.path().canonicalize().expect("canonical home");
        let path = home.join("cache/jobs.sock");
        bind_stale(&path);
        let singleton = path.parent().expect("socket parent").join("tui.lock");
        mkfifo(&singleton, Mode::from_bits_truncate(0o600)).expect("singleton fifo");
        let worker_path = path.clone();
        let worker_home = home;
        let (finished_tx, finished_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let result = remove_stale_owned_socket(&worker_path, &worker_home);
            finished_tx.send(result).expect("publish migration result");
        });

        let bounded = finished_rx.recv_timeout(Duration::from_millis(250));
        if bounded.is_err() {
            let mut writer = std::fs::OpenOptions::new()
                .write(true)
                .open(&singleton)
                .expect("unblock unsafe fifo reader");
            writer
                .write_all(std::process::id().to_string().as_bytes())
                .expect("unblock contents");
        }
        worker.join().expect("migration worker");

        let result = bounded.expect("singleton inspection exceeded its bounded budget");
        assert!(
            result.is_ok(),
            "invalid singleton inspection failed startup"
        );
        assert!(path.exists(), "invalid singleton authorized socket removal");
    }

    #[test]
    fn a_symlink_singleton_never_follows_a_blocking_target() {
        let temporary = tempfile::tempdir().expect("temporary home");
        let home = temporary.path().canonicalize().expect("canonical home");
        let path = home.join("cache/jobs.sock");
        bind_stale(&path);
        let target = home.join("blocking-target");
        mkfifo(&target, Mode::from_bits_truncate(0o600)).expect("target fifo");
        std::os::unix::fs::symlink(
            &target,
            path.parent().expect("socket parent").join("tui.lock"),
        )
        .expect("singleton symlink");
        let worker_path = path.clone();
        let worker_home = home;
        let (finished_tx, finished_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let result = remove_stale_owned_socket(&worker_path, &worker_home);
            finished_tx.send(result).expect("publish migration result");
        });

        let bounded = finished_rx.recv_timeout(Duration::from_millis(250));
        if bounded.is_err() {
            let mut writer = std::fs::OpenOptions::new()
                .write(true)
                .open(&target)
                .expect("unblock followed symlink target");
            writer
                .write_all(std::process::id().to_string().as_bytes())
                .expect("unblock contents");
        }
        worker.join().expect("migration worker");

        let result = bounded.expect("singleton symlink was followed past the bounded budget");
        assert!(
            result.is_ok(),
            "symlink singleton inspection failed startup"
        );
        assert!(path.exists(), "symlink singleton authorized socket removal");
    }

    #[test]
    fn an_oversized_singleton_never_authorizes_socket_removal() {
        let temporary = tempfile::tempdir().expect("temporary home");
        let home = temporary.path().canonicalize().expect("canonical home");
        let path = home.join("cache/jobs.sock");
        bind_stale(&path);
        std::fs::write(
            path.parent().expect("socket parent").join("tui.lock"),
            vec![b'1'; 33],
        )
        .expect("oversized singleton");

        let result = remove_stale_owned_socket(&path, &home);

        assert!(
            result.is_ok(),
            "oversized singleton inspection failed startup"
        );
        assert!(
            path.exists(),
            "oversized singleton authorized socket removal"
        );
    }

    #[test]
    fn liveness_probe_never_follows_a_raced_socket_symlink() {
        let temporary = tempfile::tempdir().expect("temporary home");
        let home = temporary.path().canonicalize().expect("canonical home");
        let path = home.join("cache/jobs.sock");
        bind_stale(&path);
        let target = home.join("target.sock");
        let listener = UnixListener::bind(&target).expect("symlink target listener");
        listener
            .set_nonblocking(true)
            .expect("nonblocking target listener");
        let raced_path = path.clone();
        let raced_target = target;

        let result = with_secure_remove_test_hook(
            move |boundary, _| {
                if boundary == SecureRemoveTestBoundary::LegacySocketIdentityObservedBeforeLiveness
                {
                    std::fs::remove_file(&raced_path).expect("retire observed socket");
                    std::os::unix::fs::symlink(&raced_target, &raced_path)
                        .expect("raced socket symlink");
                }
            },
            || remove_stale_owned_socket(&path, &home),
        );

        assert!(result.is_ok(), "raced symlink inspection failed startup");
        assert!(
            std::fs::symlink_metadata(&path)
                .is_ok_and(|metadata| metadata.file_type().is_symlink()),
            "raced symlink was removed"
        );
        let accept = listener.accept();
        assert!(
            accept
                .as_ref()
                .is_err_and(|error| error.kind() == std::io::ErrorKind::WouldBlock),
            "liveness probe followed the raced socket symlink"
        );
    }
}
