use super::{
    install_transaction::{InstallIdentity, InstallVersion, LockedInstall, NoStamp},
    transaction_tests::rendered,
};
use std::fs;

#[test]
fn legacy_adoption_retains_verified_complete_backup_and_unknown_content() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let tree = root.path().join(".agents/skills/todo");
    fs::create_dir_all(tree.join("references")).unwrap();
    fs::write(tree.join("SKILL.md"), "historically edited").unwrap();
    fs::write(tree.join(".brain-rendered"), "0.1.0-rc.1+build.3\n").unwrap();
    fs::write(tree.join("references/old.md"), "unknown old reference").unwrap();
    fs::create_dir_all(root.path().join(".claude/skills")).unwrap();
    std::os::unix::fs::symlink(&tree, root.path().join(".claude/skills/todo")).unwrap();
    let stale = root.path().join(".agents/skills/old-plugin");
    fs::create_dir_all(&stale).unwrap();
    fs::write(stale.join("SKILL.md"), "unknown stale skill").unwrap();
    fs::write(stale.join(".brain-rendered"), "0.1.0\n").unwrap();
    let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
    let mut locked = LockedInstall::acquire(&identity).unwrap();
    locked
        .install(rendered("new"), &InstallVersion::current(), &mut NoStamp)
        .unwrap();
    assert_eq!(fs::read(tree.join("SKILL.md")).unwrap(), b"new");
    assert_eq!(
        fs::read(tree.join("references/old.md")).unwrap(),
        b"unknown old reference"
    );
    assert_eq!(
        fs::read(stale.join("SKILL.md")).unwrap(),
        b"unknown stale skill"
    );
    let backup = fs::read_dir(cache.path().join("skills-install/legacy-backups"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(
        fs::read(backup.join(".agents/skills/todo/SKILL.md")).unwrap(),
        b"historically edited"
    );
    assert_eq!(
        fs::read(backup.join(".agents/skills/todo/references/old.md")).unwrap(),
        b"unknown old reference"
    );
    assert_eq!(
        fs::read_link(backup.join(".claude/skills/todo")).unwrap(),
        tree
    );
    assert_eq!(
        fs::read(backup.join(".agents/skills/old-plugin/SKILL.md")).unwrap(),
        b"unknown stale skill"
    );
    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(cache.path().join("skills-install/managed.json")).unwrap(),
    )
    .unwrap();
    assert!(
        manifest["files"]
            .get(".agents/skills/todo/references/old.md")
            .is_none()
    );
    assert!(
        manifest["files"]
            .get(".agents/skills/old-plugin/SKILL.md")
            .is_none()
    );
}

#[test]
fn malformed_and_symlink_legacy_markers_never_authorize_adoption() {
    for (symlink, marker) in [
        (false, "not a version"),
        (false, "1.2.3-???"),
        (false, "1.2.3-"),
        (false, "01.2.3"),
        (true, "0.1.0"),
    ] {
        let root = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let tree = root.path().join(".agents/skills/todo");
        fs::create_dir_all(&tree).unwrap();
        fs::write(tree.join("SKILL.md"), "mine").unwrap();
        if symlink {
            fs::write(root.path().join("version"), "0.1.0").unwrap();
            std::os::unix::fs::symlink(root.path().join("version"), tree.join(".brain-rendered"))
                .unwrap();
        } else {
            fs::write(tree.join(".brain-rendered"), marker).unwrap();
        }
        let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
        let mut locked = LockedInstall::acquire(&identity).unwrap();
        assert!(
            locked
                .install(rendered("new"), &InstallVersion::current(), &mut NoStamp)
                .is_err()
        );
        assert_eq!(fs::read(tree.join("SKILL.md")).unwrap(), b"mine");
    }
}
