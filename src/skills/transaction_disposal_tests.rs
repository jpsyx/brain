use super::{
    install_transaction::{Checkpoint, InstallIdentity, InstallVersion, LockedInstall, NoStamp},
    transaction_tests::rendered,
};
use std::{
    fs,
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
};

fn interrupted(root: &Path, cache: &Path, committed: bool) -> (InstallIdentity, serde_json::Value) {
    let identity = InstallIdentity::sandbox_with_cache(root, cache).unwrap();
    let mut locked = LockedInstall::acquire(&identity).unwrap();
    let version = InstallVersion::current();
    locked
        .install(rendered("old"), &version, &mut NoStamp)
        .unwrap();
    let stop = if committed {
        Checkpoint::ManifestPublished
    } else {
        Checkpoint::Installed(0)
    };
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _ = locked.install_with(rendered("new"), &version, &mut NoStamp, &mut |point| {
                assert!(point != stop, "simulated interruption");
                Ok(())
            });
        }))
        .is_err()
    );
    let journal =
        serde_json::from_slice(&fs::read(cache.join("skills-install/pending.json")).unwrap())
            .unwrap();
    (identity, journal)
}
fn slot(root: &Path, journal: &serde_json::Value, role: &str) -> PathBuf {
    root.join(format!(
        ".agents/skills/.brain-skills-{}-0-{role}",
        journal["transaction_id"].as_str().unwrap()
    ))
}
fn phase(cache: &Path, journal: &mut serde_json::Value, value: &str) {
    journal["phase"] = value.into();
    fs::write(
        cache.join("skills-install/pending.json"),
        serde_json::to_vec(journal).unwrap(),
    )
    .unwrap();
}

#[test]
fn rollback_resumes_after_detaching_or_restoring_the_live_tree() {
    for restored in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let (identity, mut journal) = interrupted(root.path(), cache.path(), false);
        phase(cache.path(), &mut journal, "RollingBack");
        let live = root.path().join(".agents/skills/todo");
        fs::rename(&live, slot(root.path(), &journal, "discard")).unwrap();
        if restored {
            fs::rename(slot(root.path(), &journal, "backup"), &live).unwrap();
        }
        let mut locked = LockedInstall::acquire(&identity).unwrap();
        assert!(locked.recover(&mut NoStamp).unwrap().is_none());
        assert_eq!(fs::read(live.join("SKILL.md")).unwrap(), b"old");
        assert_eq!(
            fs::read(live.join("references/guide.md")).unwrap(),
            b"reference old"
        );
        assert!(!cache.path().join("skills-install/pending.json").exists());
        assert!(!slot(root.path(), &journal, "discard").exists());
    }
}

#[test]
fn partial_disposal_resumes_after_rollback_and_commit() {
    for committed in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let (identity, mut journal) = interrupted(root.path(), cache.path(), committed);
        let live = root.path().join(".agents/skills/todo");
        let disposal = if committed {
            phase(cache.path(), &mut journal, "DisposingCommitted");
            slot(root.path(), &journal, "backup")
        } else {
            fs::rename(&live, slot(root.path(), &journal, "discard")).unwrap();
            fs::rename(slot(root.path(), &journal, "backup"), &live).unwrap();
            phase(cache.path(), &mut journal, "DisposingRollback");
            slot(root.path(), &journal, "discard")
        };
        fs::remove_file(disposal.join("references/guide.md")).unwrap();
        let mut locked = LockedInstall::acquire(&identity).unwrap();
        assert_eq!(locked.recover(&mut NoStamp).unwrap().is_some(), committed);
        assert_eq!(
            fs::read(live.join("SKILL.md")).unwrap(),
            if committed { b"new" } else { b"old" }
        );
        assert!(!disposal.exists());
        assert!(!cache.path().join("skills-install/pending.json").exists());
    }
}

#[test]
fn partial_disposal_rejects_changed_or_new_content_before_removing_anything() {
    for changed in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let (identity, mut journal) = interrupted(root.path(), cache.path(), true);
        phase(cache.path(), &mut journal, "DisposingCommitted");
        let backup = slot(root.path(), &journal, "backup");
        fs::remove_file(backup.join("references/guide.md")).unwrap();
        let protected = backup.join(if changed { "SKILL.md" } else { "unexpected" });
        fs::write(&protected, "post-crash edit").unwrap();
        let marker = fs::read(backup.join(".brain-rendered")).unwrap();
        let mut locked = LockedInstall::acquire(&identity).unwrap();
        assert!(locked.recover(&mut NoStamp).is_err());
        assert_eq!(fs::read(protected).unwrap(), b"post-crash edit");
        assert_eq!(fs::read(backup.join(".brain-rendered")).unwrap(), marker);
        assert!(cache.path().join("skills-install/pending.json").exists());
    }
}
