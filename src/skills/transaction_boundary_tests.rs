use super::{
    install::{Sources, sync_identity},
    install_transaction::{
        Checkpoint, InstallIdentity, InstallReceipt, InstallVersion, LockedInstall, NoStamp,
        StampStore,
    },
    transaction_tests::rendered,
};
use std::{
    fs,
    sync::{Arc, Barrier},
};

struct FailOnce(bool);
impl StampStore for FailOnce {
    fn write(&mut self, _: &InstallReceipt) -> anyhow::Result<()> {
        if self.0 {
            self.0 = false;
            anyhow::bail!("stamp temporarily unavailable");
        }
        Ok(())
    }
}
#[test]
fn stamp_retry_finishes_without_reading_render_sources_again() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
    let plugins = root.path().join("plugins");
    fs::create_dir_all(plugins.join("mine")).unwrap();
    fs::write(plugins.join("mine/SKILL.md"), "source").unwrap();
    let sources = Sources {
        plugins_dir: Some(plugins.clone()),
        extensions_dir: None,
    };
    let mut stamp = FailOnce(true);
    assert!(sync_identity(&identity, &sources, &mut stamp).is_err());
    fs::rename(&plugins, root.path().join("retained-source")).unwrap();
    fs::write(plugins, "unreadable source directory").unwrap();
    let skill = root.path().join(".agents/skills/mine/SKILL.md");
    let inode = std::os::unix::fs::MetadataExt::ino(&fs::metadata(&skill).unwrap());
    assert!(sync_identity(&identity, &sources, &mut stamp).is_ok());
    assert_eq!(
        std::os::unix::fs::MetadataExt::ino(&fs::metadata(skill).unwrap()),
        inode
    );
    assert!(!cache.path().join("skills-install/pending.json").exists());
}

#[test]
fn failed_legacy_backup_leaves_live_tree_and_no_completed_stages() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let tree = root.path().join(".agents/skills/todo");
    fs::create_dir_all(&tree).unwrap();
    fs::write(tree.join("SKILL.md"), "old").unwrap();
    fs::write(tree.join(".brain-rendered"), "0.1.0").unwrap();
    let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
    let mut locked = LockedInstall::acquire(&identity).unwrap();
    fs::write(
        cache.path().join("skills-install/legacy-backups"),
        "protected file",
    )
    .unwrap();
    assert!(
        locked
            .install(rendered("new"), &InstallVersion::current(), &mut NoStamp)
            .is_err()
    );
    assert_eq!(fs::read(tree.join("SKILL.md")).unwrap(), b"old");
    assert_eq!(fs::read_dir(tree.parent().unwrap()).unwrap().count(), 1);
    assert!(!cache.path().join("skills-install/managed.json").exists());
}

#[test]
fn concurrent_installers_share_one_persistent_lock_inode() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
    let locked = LockedInstall::acquire(&identity).unwrap();
    let inode = std::os::unix::fs::MetadataExt::ino(
        &fs::metadata(cache.path().join("skills-install/install.lock")).unwrap(),
    );
    let barrier = Arc::new(Barrier::new(2));
    std::thread::scope(|scope| {
        let ready = Arc::clone(&barrier);
        let identity = &identity;
        scope.spawn(move || {
            ready.wait();
            let error = LockedInstall::acquire(identity)
                .err()
                .expect("second installer cannot own the lock");
            assert!(error.to_string().contains("busy"));
        });
        barrier.wait();
    });
    drop(locked);
    let _locked = LockedInstall::acquire(&identity).unwrap();
    assert_eq!(
        std::os::unix::fs::MetadataExt::ino(
            &fs::metadata(cache.path().join("skills-install/install.lock")).unwrap()
        ),
        inode
    );
}

#[test]
fn publication_failure_finishes_the_verified_new_tree() {
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
            .install_with(rendered("new"), &version, &mut NoStamp, &mut |point| {
                anyhow::ensure!(
                    point != Checkpoint::ManifestPublished,
                    "after publication interruption"
                );
                Ok(())
            })
            .is_err()
    );
    let skill = root.path().join(".agents/skills/todo/SKILL.md");
    assert_eq!(fs::read(&skill).unwrap(), b"new");
    locked.recover(&mut NoStamp).unwrap();
    assert_eq!(fs::read(skill).unwrap(), b"new");
}

#[test]
fn new_render_validation_happens_before_any_staging() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
    let mut locked = LockedInstall::acquire(&identity).unwrap();
    let mut bad = rendered("new");
    bad.get_mut("todo")
        .unwrap()
        .insert("SKILL.md/child".into(), b"collision".to_vec());
    assert!(
        locked
            .install(bad, &InstallVersion::current(), &mut NoStamp)
            .is_err()
    );
    assert!(!root.path().join(".agents").exists());
}

#[test]
fn published_journal_rejects_a_regressed_committed_manifest() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
    let mut locked = LockedInstall::acquire(&identity).unwrap();
    let version = InstallVersion::current();
    locked
        .install(rendered("old"), &version, &mut NoStamp)
        .unwrap();
    let manifest = cache.path().join("skills-install/managed.json");
    let old = fs::read(&manifest).unwrap();
    let _ = locked.install_with(rendered("new"), &version, &mut NoStamp, &mut |point| {
        anyhow::ensure!(point != Checkpoint::ManifestPublished, "interruption");
        Ok(())
    });
    fs::write(manifest, old).unwrap();
    assert!(locked.recover(&mut NoStamp).is_err());
    assert_eq!(
        fs::read(root.path().join(".agents/skills/todo/SKILL.md")).unwrap(),
        b"new"
    );
    assert!(cache.path().join("skills-install/pending.json").exists());
}
