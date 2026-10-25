use super::{
    Checkpoint, InstallIdentity, InstallReceipt, InstallVersion, LockedInstall, NoStamp,
    StampStore,
    package::ManagedWorkspacePackage,
    package_patch_tests::{patched, seed},
    package_tests::package,
    snapshot,
};
use std::{
    fs,
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
};

fn complete(root: &Path) -> ManagedWorkspacePackage {
    let mut result = package(root, "new");
    let patches = patched(root);
    result.config_patches = patches.config_patches;
    result.source_patches = patches.source_patches;
    result
}

#[test]
fn every_package_rename_boundary_restores_the_complete_before_generation() {
    let mut points = vec![Checkpoint::Staged];
    for index in 0..7 {
        points.extend([Checkpoint::BackedUp(index), Checkpoint::Installed(index)]);
    }
    for stop in points {
        let root = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        seed(root.path());
        let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
        let mut locked = LockedInstall::acquire(&identity).unwrap();
        let version = InstallVersion::current();
        locked
            .install_package(package(root.path(), "old"), &version, &mut NoStamp)
            .unwrap();
        fs::write(
            root.path().join(".agents/skills/todo/custom.bin"),
            [0, 255, 27],
        )
        .unwrap();
        std::os::unix::fs::symlink("AGENTS.md", root.path().join("CLAUDE.md")).unwrap();
        let before = snapshot::read(root.path()).unwrap();
        let manifest = fs::read(cache.path().join("skills-install/managed.json")).unwrap();
        let error = locked
            .install_package_with(
                complete(root.path()),
                &version,
                &mut NoStamp,
                &mut |point| {
                    anyhow::ensure!(point != stop, "injected package boundary");
                    Ok(())
                },
            )
            .unwrap_err();
        assert!(
            error.to_string().contains("injected package boundary"),
            "{stop:?}: {error}"
        );
        assert!(locked.recover(&mut NoStamp).unwrap().is_none());
        assert_eq!(snapshot::read(root.path()).unwrap(), before, "{stop:?}");
        assert_eq!(
            fs::read(cache.path().join("skills-install/managed.json")).unwrap(),
            manifest
        );
    }
}
struct FailingStamp;
impl StampStore for FailingStamp {
    fn write(&mut self, _: &InstallReceipt) -> anyhow::Result<()> {
        anyhow::bail!("stamp unavailable")
    }
}

#[test]
fn publication_and_stamp_failures_finish_the_same_complete_package_without_replacement() {
    use std::os::unix::fs::MetadataExt;
    for stamp_failure in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        seed(root.path());
        let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
        let mut locked = LockedInstall::acquire(&identity).unwrap();
        let version = InstallVersion::current();
        locked
            .install_package(package(root.path(), "old"), &version, &mut NoStamp)
            .unwrap();
        let result = if stamp_failure {
            locked.install_package(complete(root.path()), &version, &mut FailingStamp)
        } else {
            locked.install_package_with(
                complete(root.path()),
                &version,
                &mut NoStamp,
                &mut |point| {
                    anyhow::ensure!(
                        point != Checkpoint::ManifestPublished,
                        "published interruption"
                    );
                    Ok(())
                },
            )
        };
        assert!(result.is_err());
        let agents = root.path().join("AGENTS.md");
        let inode = fs::metadata(&agents).unwrap().ino();
        assert!(
            fs::read_to_string(&agents)
                .unwrap()
                .contains("new instructions")
        );
        assert_eq!(
            fs::read(root.path().join(".config/extensions/todo.md")).unwrap(),
            b"Legacy user text\n\n[todo:anchors]\nCanonical anchor\n"
        );
        assert!(locked.recover(&mut NoStamp).unwrap().is_some());
        assert_eq!(fs::metadata(agents).unwrap().ino(), inode);
        assert!(!cache.path().join("skills-install/pending.json").exists());
    }
}

#[test]
fn package_disposal_resumes_after_partial_commit_and_rollback_cleanup() {
    for committed in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        seed(root.path());
        let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
        let mut locked = LockedInstall::acquire(&identity).unwrap();
        let version = InstallVersion::current();
        locked
            .install_package(package(root.path(), "old"), &version, &mut NoStamp)
            .unwrap();
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                let _ = locked.install_package_with(
                    complete(root.path()),
                    &version,
                    &mut NoStamp,
                    &mut |point| {
                        assert!(
                            point
                                != if committed {
                                    Checkpoint::ManifestPublished
                                } else {
                                    Checkpoint::Installed(6)
                                },
                            "process loss"
                        );
                        Ok(())
                    },
                );
            }))
            .is_err()
        );
        let pending = cache.path().join("skills-install/pending.json");
        let mut journal: super::journal::Journal =
            serde_json::from_slice(&fs::read(&pending).unwrap()).unwrap();
        let root_path = root.path().canonicalize().unwrap();
        if committed {
            journal
                .transition(
                    super::journal::Phase::DisposingCommitted,
                    pending.parent().unwrap(),
                )
                .unwrap();
        } else {
            journal
                .rollback(&root_path, pending.parent().unwrap())
                .unwrap();
        }
        let role = if committed { "backup" } else { "discard" };
        let config = journal
            .entries
            .iter()
            .position(|entry| entry.live == ".config/config.json")
            .unwrap();
        fs::remove_file(journal.entries[config].slot(
            &root_path,
            &journal.transaction_id,
            config,
            role,
        ))
        .unwrap();
        let before_cleanup = snapshot::read(&root_path.join("AGENTS.md")).unwrap();
        assert_eq!(locked.recover(&mut NoStamp).unwrap().is_some(), committed);
        assert_eq!(
            snapshot::read(&root_path.join("AGENTS.md")).unwrap(),
            before_cleanup
        );
        assert!(!pending.exists());
    }
}
