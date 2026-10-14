use super::{
    install_transaction::{
        Currentness, InstallBusy, InstallIdentity, InstallVersion, LockedInstall, NoStamp,
    },
    transaction_tests::rendered,
};
use std::fs;

#[test]
fn same_version_inspection_detects_missing_frontends_and_owned_edits() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
    let mut locked = LockedInstall::acquire(&identity).unwrap();
    let version = InstallVersion::current();
    locked
        .install(rendered("current"), &version, &mut NoStamp)
        .unwrap();
    assert!(matches!(
        locked.inspect(&version).unwrap(),
        Currentness::Current(_)
    ));
    fs::remove_file(root.path().join(".codex/skills/todo")).unwrap();
    assert!(
        matches!(locked.inspect(&version).unwrap(), Currentness::Repairable(paths) if paths.contains(&".codex/skills/todo".into()))
    );
    locked
        .install(rendered("current"), &version, &mut NoStamp)
        .unwrap();
    fs::write(
        root.path().join(".agents/skills/todo/references/guide.md"),
        "user edit",
    )
    .unwrap();
    assert!(locked.inspect(&version).is_err());
}

#[test]
fn preflight_rejects_invalid_version_without_staging() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
    let locked = LockedInstall::acquire(&identity).unwrap();
    let version = InstallVersion {
        binary_version: "invalid".into(),
        compatibility_revision: 0,
    };
    assert!(locked.preflight(&rendered("old target"), &version).is_err());
    assert!(!root.path().join(".agents").exists());
}

#[test]
fn changed_portable_identity_is_rejected_before_creating_install_state() {
    use crate::workspace::{WorkspaceId, WorkspaceManifest};
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let id = WorkspaceId::new();
    WorkspaceManifest::new(id).write_new(root.path()).unwrap();
    let identity = InstallIdentity::registered(root.path(), id, cache.path()).unwrap();
    fs::remove_file(WorkspaceManifest::path(root.path())).unwrap();
    WorkspaceManifest::new(WorkspaceId::new())
        .write_new(root.path())
        .unwrap();
    assert!(LockedInstall::acquire(&identity).is_err());
    assert!(!cache.path().join("skills-install").exists());
}

#[test]
fn contention_is_a_typed_busy_outcome() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
    let _first = LockedInstall::acquire(&identity).unwrap();
    let error = LockedInstall::acquire(&identity).err().unwrap();
    assert!(error.downcast_ref::<InstallBusy>().is_some());
}

#[test]
fn manifested_sandbox_and_registered_identity_use_one_injected_lock_history() {
    use crate::workspace::{WorkspaceId, WorkspaceManifest};
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let id = WorkspaceId::new();
    WorkspaceManifest::new(id).write_new(root.path()).unwrap();
    let registered = InstallIdentity::registered(root.path(), id, cache.path()).unwrap();
    let sandbox = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
    let mut locked = LockedInstall::acquire(&registered).unwrap();
    locked
        .install(
            rendered("same identity"),
            &InstallVersion::current(),
            &mut NoStamp,
        )
        .unwrap();
    assert!(LockedInstall::acquire(&sandbox).is_err());
    drop(locked);
    let locked = LockedInstall::acquire(&sandbox).unwrap();
    assert_eq!(
        locked.manifest().unwrap().unwrap().workspace_id,
        id.to_string()
    );
}

#[test]
fn missing_tree_repairs_exact_absolute_dangling_links_and_preserves_symlink_extras() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
    let mut locked = LockedInstall::acquire(&identity).unwrap();
    let version = InstallVersion::current();
    locked
        .install(rendered("old"), &version, &mut NoStamp)
        .unwrap();
    let tree = root.path().join(".agents/skills/todo");
    let frontend = root.path().join(".codex/skills/todo");
    fs::remove_file(&frontend).unwrap();
    std::os::unix::fs::symlink(&tree, &frontend).unwrap();
    fs::remove_dir_all(&tree).unwrap();
    locked
        .install(rendered("new"), &version, &mut NoStamp)
        .unwrap();
    assert_eq!(
        fs::read_link(&frontend).unwrap(),
        std::path::Path::new("../../.agents/skills/todo")
    );
    std::os::unix::fs::symlink("../../../../outside", tree.join("extra")).unwrap();
    locked
        .install(rendered("next"), &version, &mut NoStamp)
        .unwrap();
    assert_eq!(
        fs::read_link(tree.join("extra")).unwrap(),
        std::path::Path::new("../../../../outside")
    );
    let mut collision = rendered("next");
    collision
        .get_mut("todo")
        .unwrap()
        .insert("extra/new.md".into(), b"no".to_vec());
    assert!(locked.install(collision, &version, &mut NoStamp).is_err());
}
