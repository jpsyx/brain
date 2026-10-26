use super::*;

#[test]
fn released_sync_lock_during_kind_observation_can_be_acquired() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
    let owner = identity.acquire_sync().unwrap();
    let successor = identity
        .acquire_sync_observed(|| drop(owner))
        .expect("a released ephemeral lock must not become an unreadable source");
    assert!(identity.sync_active().unwrap());
    drop(successor);
    assert!(!identity.sync_active().unwrap());
}

#[test]
fn sync_lock_observation_preserves_live_owner_and_refuses_unsafe_kinds() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
    let owner = identity.acquire_sync().unwrap();
    let path = identity.sync_lock();
    let bytes = fs::read(&path).unwrap();
    assert!(identity.acquire_sync().err().unwrap().is::<SyncBusy>());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    drop(owner);

    let outside = root.path().join("outside");
    fs::write(&outside, b"preserve").unwrap();
    std::os::unix::fs::symlink(&outside, &path).unwrap();
    assert!(identity.acquire_sync().is_err());
    assert!(
        fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read(&outside).unwrap(), b"preserve");
    fs::remove_file(&path).unwrap();

    fs::create_dir(&path).unwrap();
    assert!(identity.acquire_sync().is_err());
    assert!(path.is_dir());
    fs::remove_dir(&path).unwrap();

    let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
    assert!(identity.acquire_sync().is_err());
    drop(listener);
    fs::remove_file(&path).unwrap();
    assert!(identity.acquire_sync().is_ok());
}
