use super::{
    InstallIdentity, InstallVersion, LockedInstall, NoStamp,
    package_patch_tests::{patched, seed},
    package_tests::package,
    snapshot,
};
use std::fs;

#[test]
fn package_preflight_rejects_protected_paths_and_edits_without_live_changes() {
    for conflict in [
        "forged source",
        "config field",
        "external AGENTS",
        "foreign frontend",
        "managed body",
        "legacy source",
        "canonical source",
    ] {
        let root = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        seed(root.path());
        let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
        let mut locked = LockedInstall::acquire(&identity).unwrap();
        let version = InstallVersion::current();
        locked
            .install_package(package(root.path(), "old"), &version, &mut NoStamp)
            .unwrap();
        let mut desired = package(root.path(), "new");
        let patches = patched(root.path());
        desired.config_patches = patches.config_patches;
        desired.source_patches = patches.source_patches;
        match conflict {
            "forged source" => {
                desired.source_patches[0].relative_path = ".config/users.json".into();
            }
            "config field" => desired.config_patches[0].field = "theme".into(),
            "external AGENTS" => {
                fs::write(outside.path().join("AGENTS.md"), "outside").unwrap();
                fs::remove_file(root.path().join("AGENTS.md")).unwrap();
                std::os::unix::fs::symlink(
                    outside.path().join("AGENTS.md"),
                    root.path().join("AGENTS.md"),
                )
                .unwrap();
            }
            "foreign frontend" => {
                fs::remove_file(root.path().join(".codex/skills/todo")).unwrap();
                fs::write(root.path().join(".codex/skills/todo"), "foreign").unwrap();
            }
            "managed body" => {
                let file = root.path().join("AGENTS.md");
                fs::write(
                    &file,
                    fs::read_to_string(&file)
                        .unwrap()
                        .replace("old instructions", "user edit"),
                )
                .unwrap();
            }
            "legacy source" => fs::write(
                root.path().join(".config/extensions/todo.md"),
                "legacy edit",
            )
            .unwrap(),
            "canonical source" => fs::write(
                root.path().join(".config/extensions/agenda.md"),
                "canonical edit",
            )
            .unwrap(),
            _ => unreachable!(),
        }
        let before = snapshot::read(root.path()).unwrap();
        assert!(
            locked.preflight_package(&desired, &version).is_err(),
            "{conflict}"
        );
        assert!(
            locked
                .install_package(desired, &version, &mut NoStamp)
                .is_err(),
            "{conflict}"
        );
        assert_eq!(snapshot::read(root.path()).unwrap(), before, "{conflict}");
        assert!(!cache.path().join("skills-install/pending.json").exists());
    }
}

#[test]
fn unowned_instruction_edit_after_staging_is_detected_before_any_live_artifact_moves() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
    let mut locked = LockedInstall::acquire(&identity).unwrap();
    let version = InstallVersion::current();
    locked
        .install_package(package(root.path(), "old"), &version, &mut NoStamp)
        .unwrap();
    let next = package(root.path(), "new");
    let agents = root.path().join("AGENTS.md");
    assert!(
        locked
            .install_package_with(next, &version, &mut NoStamp, &mut |point| {
                if point == super::Checkpoint::Staged {
                    let mut bytes = fs::read(&agents)?;
                    bytes.extend_from_slice(b"concurrent user suffix\n");
                    fs::write(&agents, bytes)?;
                }
                Ok(())
            })
            .is_err()
    );
    assert_eq!(
        fs::read(root.path().join(".agents/skills/todo/SKILL.md")).unwrap(),
        b"old"
    );
    assert!(
        fs::read_to_string(agents)
            .unwrap()
            .ends_with("concurrent user suffix\n")
    );
    assert!(cache.path().join("skills-install/pending.json").exists());
}

#[test]
fn staged_and_backup_file_collisions_never_follow_a_preexisting_symlink() {
    use std::collections::BTreeMap;
    for backup in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let source = root.path().join("AGENTS.md");
        fs::write(&source, b"new artifact").unwrap();
        let target = outside.path().join("protected");
        fs::write(&target, b"protected bytes").unwrap();
        let stage = root.path().join("stage");
        std::os::unix::fs::symlink(&target, &stage).unwrap();
        let fingerprint = snapshot::read(&source).unwrap();
        let result = if backup {
            snapshot::copy(&source, &stage, &fingerprint)
        } else {
            snapshot::materialize(
                root.path(),
                &stage,
                "AGENTS.md",
                &fingerprint,
                &BTreeMap::from([("AGENTS.md".into(), b"new artifact".to_vec())]),
            )
        };
        assert!(result.is_err());
        assert_eq!(
            fs::read(&target).unwrap(),
            b"protected bytes",
            "a rejected artifact must not have followed its destination"
        );
        assert!(
            fs::symlink_metadata(stage)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
}
