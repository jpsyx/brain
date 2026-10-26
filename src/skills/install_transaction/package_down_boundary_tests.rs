//! Every legacy-package publication boundary shares the current transaction recovery.
use super::{
    Checkpoint, InstallIdentity, InstallReceipt, InstallVersion, LockedInstall, NoStamp,
    StampStore,
    package::{InstructionRestore, LegacyWorkspacePackage, TargetInstructionContract},
    package_patch_tests::{patched, seed},
    package_tests::package,
    snapshot,
};
use std::fs;

struct FailingStamp;
impl StampStore for FailingStamp {
    fn write(&mut self, _: &InstallReceipt) -> anyhow::Result<()> {
        anyhow::bail!("stamp unavailable")
    }
}

#[test]
fn complete_down_recovers_instructions_skills_config_sources_manifest_and_stamp_boundaries() {
    let mut points = vec![Some(Checkpoint::Staged)];
    for index in 0..7 {
        points.extend([
            Some(Checkpoint::BackedUp(index)),
            Some(Checkpoint::Installed(index)),
        ]);
    }
    points.extend([Some(Checkpoint::ManifestPublished), None]);
    for stop in points {
        let root = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        seed(root.path());
        let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
        let mut locked = LockedInstall::acquire(&identity).unwrap();
        locked
            .install_package(
                package(root.path(), "old"),
                &InstallVersion::current(),
                &mut NoStamp,
            )
            .unwrap();
        fs::write(
            root.path().join(".agents/skills/todo/user.bin"),
            [0, 255, 128],
        )
        .unwrap();
        let before = snapshot::read(root.path()).unwrap();
        let manifest = locked.manifest().unwrap().unwrap();
        let owned = manifest
            .package
            .as_ref()
            .unwrap()
            .instructions
            .as_ref()
            .unwrap();
        let instructions = fs::read(root.path().join("AGENTS.md")).unwrap();
        let contract = TargetInstructionContract {
            binary_version: "0.105.3".into(),
            bytes: b"Legacy target core\n".to_vec(),
        };
        let after =
            super::package::compose_target_instructions(Some(&instructions), owned, &contract)
                .unwrap();
        let patches = patched(root.path());
        let expected_source = patches.source_patches[0].after.clone();
        let down = LegacyWorkspacePackage {
            skills: crate::skills::transaction_tests::rendered("legacy"),
            instructions: Some(InstructionRestore {
                before: Some(instructions),
                after: Some(after.clone()),
                target_contract: Some(contract),
            }),
            config_patches: patches.config_patches,
            source_patches: patches.source_patches,
        };
        let version = InstallVersion {
            binary_version: "0.105.3".into(),
            compatibility_revision: 1,
        };
        let result = if let Some(stop) = stop {
            locked.install_legacy_package_with(down, &version, &mut NoStamp, &mut |point| {
                anyhow::ensure!(point != stop, "injected legacy boundary");
                Ok(())
            })
        } else {
            locked.install_legacy_package(down, &version, &mut FailingStamp)
        };
        assert!(result.is_err(), "checkpoint was not reached: {stop:?}");
        let committed = stop.is_none() || stop == Some(Checkpoint::ManifestPublished);
        assert_eq!(locked.recover(&mut NoStamp).unwrap().is_some(), committed);
        let observed = locked.manifest().unwrap().unwrap();
        if committed {
            assert_eq!(observed.schema_version, 1);
            assert!(observed.package.is_none());
            assert_eq!(fs::read(root.path().join("AGENTS.md")).unwrap(), after);
            assert_eq!(
                fs::read(root.path().join(".config/extensions/todo.md")).unwrap(),
                expected_source
            );
            assert_eq!(
                fs::read(root.path().join(".agents/skills/todo/SKILL.md")).unwrap(),
                b"legacy"
            );
            let config: serde_json::Value =
                serde_json::from_slice(&fs::read(root.path().join(".config/config.json")).unwrap())
                    .unwrap();
            assert_eq!(
                config["allowed_skills"],
                serde_json::json!(["organize", "todo"])
            );
            assert_eq!(config["theme"], "custom");
        } else {
            assert_eq!(observed, manifest);
            assert_eq!(snapshot::read(root.path()).unwrap(), before, "{stop:?}");
        }
        assert_eq!(
            fs::read(root.path().join(".agents/skills/todo/user.bin")).unwrap(),
            [0, 255, 128]
        );
        assert!(!cache.path().join("skills-install/pending.json").exists());
    }
}
