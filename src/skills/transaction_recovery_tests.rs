use super::{
    install_transaction::{
        Checkpoint, InstallIdentity, InstallReceipt, InstallVersion, LockedInstall, NoStamp,
        StampStore,
    },
    transaction_tests::rendered,
};
use std::{
    fs,
    panic::{AssertUnwindSafe, catch_unwind},
};

fn interrupted(root: &std::path::Path, cache: &std::path::Path, stop: usize) -> InstallIdentity {
    let identity = InstallIdentity::sandbox_with_cache(root, cache).unwrap();
    let mut locked = LockedInstall::acquire(&identity).unwrap();
    let version = InstallVersion::current();
    locked
        .install(rendered("old"), &version, &mut NoStamp)
        .unwrap();
    let panic = catch_unwind(AssertUnwindSafe(|| {
        let _ = locked.install_with(rendered("new"), &version, &mut NoStamp, &mut |point| {
            assert!(
                point != Checkpoint::Installed(stop),
                "simulated process loss"
            );
            Ok(())
        });
    }));
    assert!(panic.is_err());
    identity
}

#[test]
fn recovery_refuses_forged_paths_and_keeps_journal() {
    for replacement in [
        "../victim",
        ".config/plugins/todo",
        ".agents/skills/todo/nested",
    ] {
        let root = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let identity = interrupted(root.path(), cache.path(), 0);
        let pending = cache.path().join("skills-install/pending.json");
        let mut journal: serde_json::Value =
            serde_json::from_slice(&fs::read(&pending).unwrap()).unwrap();
        journal["entries"][0]["live"] = replacement.into();
        journal["entries"][0]["before"] = "Absent".into();
        journal["entries"][0]["after"] = "Absent".into();
        fs::write(&pending, serde_json::to_vec(&journal).unwrap()).unwrap();
        let mut locked = LockedInstall::acquire(&identity).unwrap();
        assert!(
            locked.recover(&mut NoStamp).is_err(),
            "forged path must fail"
        );
        assert!(pending.is_file());
    }
}

#[test]
fn recovery_authenticates_every_entry_before_restoring_anything() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let identity = interrupted(root.path(), cache.path(), 3);
    let skill = root.path().join(".agents/skills/todo/SKILL.md");
    fs::write(&skill, "post-crash edit").unwrap();
    let frontend = root.path().join(".codex/skills");
    let count = fs::read_dir(&frontend).unwrap().count();
    let mut locked = LockedInstall::acquire(&identity).unwrap();
    assert!(locked.recover(&mut NoStamp).is_err());
    assert_eq!(fs::read(skill).unwrap(), b"post-crash edit");
    assert_eq!(
        fs::read_dir(&frontend).unwrap().count(),
        count,
        "recovery must authenticate all artifacts before any rollback"
    );
    assert!(cache.path().join("skills-install/pending.json").is_file());
}

#[test]
fn every_rename_boundary_restores_the_complete_original_install() {
    let points = [
        Checkpoint::Staged,
        Checkpoint::BackedUp(0),
        Checkpoint::Installed(0),
        Checkpoint::BackedUp(1),
        Checkpoint::Installed(1),
        Checkpoint::BackedUp(2),
        Checkpoint::Installed(2),
        Checkpoint::BackedUp(3),
        Checkpoint::Installed(3),
    ];
    for point in points {
        let root = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
        let mut locked = LockedInstall::acquire(&identity).unwrap();
        let version = InstallVersion::current();
        locked
            .install(rendered("old"), &version, &mut NoStamp)
            .unwrap();
        let manifest = fs::read(cache.path().join("skills-install/managed.json")).unwrap();
        assert!(
            locked
                .install_with(rendered("new"), &version, &mut NoStamp, &mut |observed| {
                    anyhow::ensure!(point != observed, "injected fault");
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(
            fs::read(cache.path().join("skills-install/managed.json")).unwrap(),
            manifest
        );
        for parent in [".agents", ".claude", ".codex", ".opencode"] {
            assert_eq!(
                fs::read(
                    root.path()
                        .join(parent)
                        .join("skills/todo/references/guide.md")
                )
                .unwrap(),
                b"reference old"
            );
            assert!(
                fs::read_dir(root.path().join(parent).join("skills"))
                    .unwrap()
                    .all(|e| !e
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".brain-skills-")),
                "staging/backup residue at {point:?}"
            );
        }
    }
}

#[test]
fn same_manifest_interruption_is_recoverable() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
    let mut locked = LockedInstall::acquire(&identity).unwrap();
    let version = InstallVersion::current();
    locked
        .install(rendered("old"), &version, &mut NoStamp)
        .unwrap();
    let _ = locked.install_with(rendered("old"), &version, &mut NoStamp, &mut |point| {
        anyhow::ensure!(
            point != Checkpoint::BackedUp(0),
            "interrupted identical install"
        );
        Ok(())
    });
    assert_eq!(
        fs::read(root.path().join(".agents/skills/todo/SKILL.md")).unwrap(),
        b"old",
        "synchronous failure must restore before returning"
    );
    locked.recover(&mut NoStamp).unwrap();
    assert_eq!(
        fs::read(root.path().join(".agents/skills/todo/SKILL.md")).unwrap(),
        b"old"
    );
}

struct FailingStamp;
impl StampStore for FailingStamp {
    fn write(&mut self, _: &InstallReceipt) -> anyhow::Result<()> {
        anyhow::bail!("stamp unavailable")
    }
}
#[test]
fn committed_stamp_failure_finishes_without_rolling_back_installed_content() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
    let mut locked = LockedInstall::acquire(&identity).unwrap();
    let version = InstallVersion::current();
    locked
        .install(rendered("old"), &version, &mut NoStamp)
        .unwrap();
    assert!(
        locked
            .install(rendered("new"), &version, &mut FailingStamp)
            .is_err()
    );
    let skill = root.path().join(".agents/skills/todo/SKILL.md");
    assert_eq!(fs::read(&skill).unwrap(), b"new");
    assert!(cache.path().join("skills-install/pending.json").exists());
    let inode = std::os::unix::fs::MetadataExt::ino(&fs::metadata(&skill).unwrap());
    locked.recover(&mut NoStamp).unwrap();
    assert_eq!(
        std::os::unix::fs::MetadataExt::ino(&fs::metadata(&skill).unwrap()),
        inode
    );
    assert!(!cache.path().join("skills-install/pending.json").exists());
}

#[test]
fn identical_manifest_complete_state_finishes_but_tampered_state_is_retained() {
    for edited in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
        let mut locked = LockedInstall::acquire(&identity).unwrap();
        let version = InstallVersion::current();
        locked
            .install(rendered("old"), &version, &mut NoStamp)
            .unwrap();
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let _ = locked.install_with(rendered("old"), &version, &mut NoStamp, &mut |point| {
                assert!(
                    point != Checkpoint::Installed(3),
                    "interrupted before equivalent manifest publication"
                );
                Ok(())
            });
        }));
        let skill = root.path().join(".agents/skills/todo/SKILL.md");
        if edited {
            fs::write(&skill, "post-crash edit").unwrap();
        }
        let result = locked.recover(&mut NoStamp);
        assert_eq!(result.is_err(), edited);
        assert_eq!(
            fs::read(skill).unwrap(),
            if edited {
                b"post-crash edit".as_slice()
            } else {
                b"old".as_slice()
            }
        );
        assert_eq!(
            cache.path().join("skills-install/pending.json").exists(),
            edited
        );
    }
}

#[test]
fn recovery_refuses_retargeted_ancestors_without_touching_the_new_target() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let identity = interrupted(root.path(), cache.path(), 0);
    fs::create_dir(outside.path().join("skills")).unwrap();
    fs::write(outside.path().join("skills/todo"), "protected").unwrap();
    fs::rename(
        root.path().join(".codex"),
        root.path().join("retained-codex"),
    )
    .unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join(".codex")).unwrap();
    let mut locked = LockedInstall::acquire(&identity).unwrap();
    assert!(locked.recover(&mut NoStamp).is_err());
    assert_eq!(
        fs::read(outside.path().join("skills/todo")).unwrap(),
        b"protected"
    );
    assert!(cache.path().join("skills-install/pending.json").exists());
}
