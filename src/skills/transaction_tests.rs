use super::install_transaction::{
    Checkpoint, InstallIdentity, InstallVersion, LockedInstall, NoStamp,
};
use std::{collections::BTreeMap, fs};

pub(super) fn rendered(body: &str) -> BTreeMap<String, BTreeMap<String, Vec<u8>>> {
    BTreeMap::from([(
        "todo".into(),
        BTreeMap::from([
            ("SKILL.md".into(), body.as_bytes().to_vec()),
            (
                "references/guide.md".into(),
                format!("reference {body}").into_bytes(),
            ),
        ]),
    )])
}

#[test]
fn managed_skill_install_recovers_complete_todo_after_tree_replacement_failure() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
    let version = InstallVersion::current();
    let mut locked = LockedInstall::acquire(&identity).unwrap();
    locked
        .install(rendered("old"), &version, &mut NoStamp)
        .unwrap();
    let error = locked
        .install_with(rendered("new"), &version, &mut NoStamp, &mut |point| {
            if matches!(point, Checkpoint::Installed(0)) {
                anyhow::bail!("injected interruption");
            }
            Ok(())
        })
        .unwrap_err();
    assert!(error.to_string().contains("injected interruption"));
    locked.recover(&mut NoStamp).unwrap();
    assert_eq!(
        fs::read(root.path().join(".agents/skills/todo/SKILL.md")).unwrap(),
        b"old"
    );
    assert_eq!(
        fs::read(root.path().join(".agents/skills/todo/references/guide.md")).unwrap(),
        b"reference old"
    );
    for frontend in [".claude", ".codex", ".opencode"] {
        let link = root.path().join(frontend).join("skills/todo");
        assert_eq!(fs::read(link.join("SKILL.md")).unwrap(), b"old");
        assert_eq!(
            fs::read_link(link).unwrap(),
            std::path::Path::new("../../.agents/skills/todo")
        );
    }
}

#[test]
fn owned_file_edits_are_conflicts_but_missing_files_are_repaired() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
    let mut locked = LockedInstall::acquire(&identity).unwrap();
    let version = InstallVersion::current();
    locked
        .install(rendered("old"), &version, &mut NoStamp)
        .unwrap();
    let guide = root.path().join(".agents/skills/todo/references/guide.md");
    fs::write(&guide, "my edit").unwrap();
    assert!(
        locked
            .install(rendered("new"), &version, &mut NoStamp)
            .is_err(),
        "edited managed reference must conflict"
    );
    assert_eq!(fs::read(&guide).unwrap(), b"my edit");
    fs::remove_file(&guide).unwrap();
    locked
        .install(rendered("new"), &version, &mut NoStamp)
        .unwrap();
    assert_eq!(fs::read(guide).unwrap(), b"reference new");
}

#[test]
fn preserves_extras_and_refuses_new_generated_path_claiming_an_extra() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
    let mut locked = LockedInstall::acquire(&identity).unwrap();
    let version = InstallVersion::current();
    locked
        .install(rendered("old"), &version, &mut NoStamp)
        .unwrap();
    let extra = root.path().join(".agents/skills/todo/local.txt");
    fs::write(&extra, "mine").unwrap();
    fs::set_permissions(&extra, fs::Permissions::from_mode(0o600)).unwrap();
    locked
        .install(rendered("new"), &version, &mut NoStamp)
        .unwrap();
    assert_eq!(fs::read(&extra).unwrap(), b"mine");
    assert_eq!(
        fs::metadata(&extra).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let mut next = rendered("next");
    next.get_mut("todo")
        .unwrap()
        .insert("local.txt".into(), b"mine".to_vec());
    assert!(locked.install(next, &version, &mut NoStamp).is_err());
    locked
        .install(BTreeMap::new(), &version, &mut NoStamp)
        .unwrap();
    assert_eq!(fs::read(extra).unwrap(), b"mine");
    assert!(!root.path().join(".agents/skills/todo/SKILL.md").exists());
    assert!(
        !root
            .path()
            .join(".agents/skills/todo/.brain-rendered")
            .exists()
    );
    assert!(fs::symlink_metadata(root.path().join(".claude/skills/todo")).is_err());
}

#[test]
fn unmarked_tree_and_foreign_frontend_collisions_are_protected() {
    for collision in [".agents/skills/todo/SKILL.md", ".codex/skills/todo"] {
        let root = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let path = root.path().join(collision);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "mine").unwrap();
        let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
        let mut locked = LockedInstall::acquire(&identity).unwrap();
        assert!(
            locked
                .install(rendered("new"), &InstallVersion::current(), &mut NoStamp)
                .is_err()
        );
        assert_eq!(fs::read(path).unwrap(), b"mine");
    }
}
