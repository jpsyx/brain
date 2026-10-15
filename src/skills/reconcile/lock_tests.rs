use super::*;
use crate::skills::{
    install_transaction::{Currentness, InstallVersion, NoStamp},
    transaction_tests::rendered,
};
use std::{cell::Cell, fs};

fn acquire_until(
    identity: &InstallIdentity,
    deadline: Instant,
    now: impl FnMut() -> Instant,
    poll: impl FnMut(Duration),
) -> Result<LockedInstall> {
    acquire_until_with_notice(identity, deadline, now, poll, || {})
}

fn fixture() -> (tempfile::TempDir, tempfile::TempDir, InstallIdentity) {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
    (root, cache, identity)
}

#[test]
fn release_before_deadline_reuses_inode_and_inspects_latest_published_receipt() {
    use std::os::unix::fs::MetadataExt as _;
    let (root, cache, identity) = fixture();
    let version = InstallVersion::current();
    let mut owner = LockedInstall::acquire(&identity).unwrap();
    let old = owner
        .install(rendered("old"), &version, &mut NoStamp)
        .unwrap();
    let inode = fs::metadata(cache.path().join("skills-install/install.lock"))
        .unwrap()
        .ino();
    let mut owner = Some(owner);
    let mut published = None;
    let start = Instant::now();
    let clock = Cell::new(start);
    let locked = acquire_until(
        &identity,
        start + WAIT,
        || clock.get(),
        |delay| {
            published = Some(
                owner
                    .as_mut()
                    .unwrap()
                    .install(rendered("new"), &version, &mut NoStamp)
                    .unwrap(),
            );
            drop(owner.take());
            clock.set(clock.get() + delay);
        },
    )
    .unwrap();
    let latest = published.unwrap();
    assert_ne!(latest, old);
    assert_eq!(
        locked.inspect(&version).unwrap(),
        Currentness::Current(latest)
    );
    assert_eq!(
        fs::read(root.path().join(".agents/skills/todo/SKILL.md")).unwrap(),
        b"new"
    );
    assert_eq!(
        fs::metadata(cache.path().join("skills-install/install.lock"))
            .unwrap()
            .ino(),
        inode
    );
}

#[test]
fn held_lock_expires_at_one_deadline_without_domain_writes() {
    let (root, cache, identity) = fixture();
    let mut owner = LockedInstall::acquire(&identity).unwrap();
    owner
        .install(
            rendered("preserved"),
            &InstallVersion::current(),
            &mut NoStamp,
        )
        .unwrap();
    let manifest = cache.path().join("skills-install/managed.json");
    let before = fs::read(&manifest).unwrap();
    let start = Instant::now();
    let clock = Cell::new(start);
    let mut polls = Vec::new();
    let error = acquire_until(
        &identity,
        start + Duration::from_millis(30),
        || clock.get(),
        |delay| {
            polls.push(delay);
            clock.set(clock.get() + delay);
        },
    )
    .err()
    .unwrap();
    assert!(error.downcast_ref::<LockTimeout>().is_some());
    assert_eq!(polls, [POLL, Duration::from_millis(5)]);
    assert_eq!(fs::read(manifest).unwrap(), before);
    assert_eq!(
        fs::read(root.path().join(".agents/skills/todo/SKILL.md")).unwrap(),
        b"preserved"
    );
    assert!(!cache.path().join("skills-install/pending.json").exists());
}

#[test]
fn elapsed_deadline_does_not_acquire_or_poll_even_with_free_lock() {
    let (_root, cache, identity) = fixture();
    let start = Instant::now();
    let error = acquire_until(
        &identity,
        start,
        || start,
        |_| panic!("no poll after expiry"),
    )
    .err()
    .unwrap();
    assert!(error.downcast_ref::<LockTimeout>().is_some());
    assert!(!cache.path().join("skills-install").exists());
}

#[test]
fn deadline_at_success_boundary_drops_ownership_before_any_work() {
    let (_root, cache, identity) = fixture();
    let start = Instant::now();
    let mut times = [start, start + WAIT].into_iter();
    let error = acquire_until(
        &identity,
        start + WAIT,
        || times.next().unwrap(),
        |_| panic!("free lock needs no poll"),
    )
    .err()
    .unwrap();
    assert!(error.downcast_ref::<LockTimeout>().is_some());
    assert!(!cache.path().join("skills-install/managed.json").exists());
    assert!(!cache.path().join("skills-install/pending.json").exists());
    let _released = LockedInstall::acquire(&identity).unwrap();
}

#[test]
fn unsafe_lock_path_is_nonbusy_and_never_polled() {
    let (_root, cache, identity) = fixture();
    fs::create_dir_all(cache.path().join("skills-install/install.lock")).unwrap();
    let start = Instant::now();
    let error = acquire_until(
        &identity,
        start + WAIT,
        || start,
        |_| panic!("nonbusy error must propagate"),
    )
    .err()
    .unwrap();
    assert!(error.to_string().contains("unsafe skills lock file"));
    assert!(error.downcast_ref::<LockTimeout>().is_none());
}

#[test]
fn changed_portable_identity_while_waiting_is_refused_before_install_work() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let id = crate::workspace::WorkspaceId::new();
    crate::workspace::WorkspaceManifest::new(id)
        .write_new(root.path())
        .unwrap();
    let identity = InstallIdentity::registered(root.path(), id, cache.path()).unwrap();
    let mut owner = Some(LockedInstall::acquire(&identity).unwrap());
    let start = Instant::now();
    let clock = Cell::new(start);
    let mut polls = 0;
    let error = acquire_until(
        &identity,
        start + WAIT,
        || clock.get(),
        |delay| {
            polls += 1;
            let path = crate::workspace::WorkspaceManifest::path(root.path());
            let mut manifest: serde_json::Value =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            manifest["workspace_id"] = serde_json::json!(crate::workspace::WorkspaceId::new());
            fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
            drop(owner.take());
            clock.set(clock.get() + delay);
        },
    )
    .err()
    .unwrap();
    assert_eq!(polls, 1);
    assert!(error.to_string().contains("identity changed"));
    assert!(!cache.path().join("skills-install/managed.json").exists());
    assert!(!cache.path().join("skills-install/pending.json").exists());
}

#[test]
fn explicit_refresh_retains_immediate_typed_busy_semantics() {
    let (root, cache, identity) = fixture();
    let _owner = LockedInstall::acquire(&identity).unwrap();
    let id = crate::workspace::WorkspaceId::new();
    let paths = crate::workspace::WorkspacePaths::new(cache.path(), id);
    let mut stamp = crate::skills::receipt::WorkspaceStamp::registered(&paths, id, "test-user");
    let error = crate::skills::reconcile::refresh(
        &identity,
        &crate::skills::reconcile::sources(root.path()),
        &mut stamp,
        crate::skills::reconcile::RefreshMode::Explicit,
    )
    .unwrap_err();
    assert!(error.downcast_ref::<InstallBusy>().is_some());
    assert!(!paths.state_db().exists());
}

#[test]
fn notice_occurs_once_on_first_typed_busy_and_never_on_free_or_nonbusy_paths() {
    let (_root, cache, identity) = fixture();
    let owner = LockedInstall::acquire(&identity).unwrap();
    let start = Instant::now();
    let clock = Cell::new(start);
    let notices = Cell::new(0);
    let result = acquire_until_with_notice(
        &identity,
        start + Duration::from_millis(60),
        || clock.get(),
        |delay| clock.set(clock.get() + delay),
        || notices.set(notices.get() + 1),
    );
    assert!(result.is_err());
    assert_eq!(notices.get(), 1);
    drop(owner);
    let locked = acquire_until_with_notice(
        &identity,
        start + WAIT,
        || start,
        |_| panic!("uncontended lock"),
        || panic!("uncontended notice"),
    )
    .unwrap();
    drop(locked);
    std::fs::remove_file(cache.path().join("skills-install/install.lock")).unwrap();
    std::fs::create_dir(cache.path().join("skills-install/install.lock")).unwrap();
    assert!(
        acquire_until_with_notice(
            &identity,
            start + WAIT,
            || start,
            |_| panic!("nonbusy poll"),
            || panic!("nonbusy notice")
        )
        .is_err()
    );
}
