use super::{
    InstallIdentity, InstallVersion, LockedInstall, NoStamp,
    package::{InstructionRestore, LegacyWorkspacePackage, ManagedWorkspacePackage},
};
use crate::{skills::transaction_tests::rendered, workspace::instructions::plan_document};
use std::fs;

#[test]
fn down_commits_restored_instructions_and_a_real_schema_one_manifest_together() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let original = b"User instructions\n";
    fs::write(root.path().join("AGENTS.md"), original).unwrap();
    let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
    let mut locked = LockedInstall::acquire(&identity).unwrap();
    locked
        .install_package(
            ManagedWorkspacePackage {
                extension_snapshot: None,
                skills: rendered("modern"),
                instructions: Some(plan_document(Some(original), "modern rules", None).unwrap()),
                config_patches: Vec::new(),
                source_patches: Vec::new(),
            },
            &InstallVersion::current(),
            &mut NoStamp,
        )
        .unwrap();
    let package = LegacyWorkspacePackage {
        skills: rendered("legacy"),
        instructions: Some(InstructionRestore {
            target_contract: None,
            before: Some(fs::read(root.path().join("AGENTS.md")).unwrap()),
            after: Some(original.to_vec()),
        }),
        config_patches: Vec::new(),
        source_patches: Vec::new(),
    };
    locked
        .install_legacy_package(
            package,
            &InstallVersion {
                binary_version: "0.105.3".into(),
                compatibility_revision: 1,
            },
            &mut NoStamp,
        )
        .unwrap();
    assert_eq!(fs::read(root.path().join("AGENTS.md")).unwrap(), original);
    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(cache.path().join("skills-install/managed.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["schema_version"], 1);
    let keys: Vec<_> = manifest
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "binary_version",
            "compatibility_revision",
            "files",
            "schema_version",
            "workspace_id"
        ]
    );
    assert!(!cache.path().join("skills-install/pending.json").exists());
    assert_eq!(
        fs::read(root.path().join(".agents/skills/todo/SKILL.md")).unwrap(),
        b"legacy"
    );
    let backups = cache.path().join("skills-install/package-backups");
    assert_eq!(fs::read_dir(backups).unwrap().count(), 1);
}

#[test]
fn down_removes_a_supported_region_adopted_from_another_machine() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let synced = plan_document(Some(b"User prefix\n"), "modern rules", None)
        .unwrap()
        .after;
    fs::write(root.path().join("AGENTS.md"), &synced).unwrap();
    let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
    let mut locked = LockedInstall::acquire(&identity).unwrap();
    locked
        .install_package(
            ManagedWorkspacePackage {
                extension_snapshot: None,
                skills: rendered("modern"),
                instructions: Some(plan_document(Some(&synced), "modern rules", None).unwrap()),
                config_patches: Vec::new(),
                source_patches: Vec::new(),
            },
            &InstallVersion::current(),
            &mut NoStamp,
        )
        .unwrap();
    let manifest = locked.manifest().unwrap().unwrap();
    let owned = manifest.package.unwrap().instructions.unwrap();
    let restored = super::package::restore_instructions(Some(&synced), &owned).unwrap();
    assert_eq!(
        restored.as_deref(),
        Some(b"User prefix\n".as_slice()),
        "adopted generated content must not become user-owned on down"
    );
}

#[test]
fn down_boundaries_restore_modern_or_finish_legacy_and_require_explicit_instruction_intent() {
    for stop in [
        super::Checkpoint::Staged,
        super::Checkpoint::BackedUp(4),
        super::Checkpoint::Installed(4),
        super::Checkpoint::ManifestPublished,
    ] {
        let root = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
        let mut locked = LockedInstall::acquire(&identity).unwrap();
        locked
            .install_package(
                super::package_tests::package(root.path(), "old"),
                &InstallVersion::current(),
                &mut NoStamp,
            )
            .unwrap();
        let before = fs::read(root.path().join("AGENTS.md")).unwrap();
        let target = InstallVersion {
            binary_version: "0.105.3".into(),
            compatibility_revision: 1,
        };
        let omitted = LegacyWorkspacePackage {
            skills: rendered("legacy"),
            instructions: None,
            config_patches: Vec::new(),
            source_patches: Vec::new(),
        };
        assert!(locked.preflight_legacy_package(&omitted, &target).is_err());
        assert_eq!(fs::read(root.path().join("AGENTS.md")).unwrap(), before);
        let down = LegacyWorkspacePackage {
            instructions: Some(InstructionRestore {
                target_contract: None,
                before: Some(before.clone()),
                after: None,
            }),
            ..omitted
        };
        locked.preflight_legacy_package(&down, &target).unwrap();
        assert!(
            locked
                .install_legacy_package_with(down, &target, &mut NoStamp, &mut |point| {
                    anyhow::ensure!(point != stop, "down interruption");
                    Ok(())
                })
                .is_err()
        );
        let committed = stop == super::Checkpoint::ManifestPublished;
        assert_eq!(locked.recover(&mut NoStamp).unwrap().is_some(), committed);
        assert_eq!(
            fs::read(root.path().join("AGENTS.md")).ok(),
            if committed { None } else { Some(before) }
        );
        let manifest = locked.manifest().unwrap().unwrap();
        assert_eq!(manifest.schema_version, if committed { 1 } else { 2 });
        assert_eq!(
            fs::read(root.path().join(".agents/skills/todo/SKILL.md")).unwrap(),
            if committed {
                b"legacy".as_slice()
            } else {
                b"old".as_slice()
            }
        );
        assert!(!cache.path().join("skills-install/pending.json").exists());
    }
}

#[test]
fn down_preserves_later_unowned_text_and_refuses_ambiguous_legacy_span_restoration() {
    for original in [
        b"User prefix\n".as_slice(),
        include_bytes!("../../../templates/workspace/legacy/AGENTS-v0.105.3.md").as_slice(),
    ] {
        let root = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
        let mut locked = LockedInstall::acquire(&identity).unwrap();
        fs::write(root.path().join("AGENTS.md"), original).unwrap();
        locked
            .install_package(
                ManagedWorkspacePackage {
                    extension_snapshot: None,
                    skills: rendered("modern"),
                    instructions: Some(
                        plan_document(Some(original), "modern rules", None).unwrap(),
                    ),
                    config_patches: Vec::new(),
                    source_patches: Vec::new(),
                },
                &InstallVersion::current(),
                &mut NoStamp,
            )
            .unwrap();
        let mut current = fs::read(root.path().join("AGENTS.md")).unwrap();
        current.extend_from_slice(b"Later user suffix\n");
        let owned = locked
            .manifest()
            .unwrap()
            .unwrap()
            .package
            .unwrap()
            .instructions
            .unwrap();
        let result = super::package::restore_instructions(Some(&current), &owned);
        if original == b"User prefix\n" {
            assert_eq!(
                result.unwrap().unwrap(),
                b"User prefix\nLater user suffix\n"
            );
        } else {
            assert!(result.is_err());
        }
    }
}

#[test]
fn target_contract_replaces_only_owned_frame_or_preserves_surviving_text_when_absent() {
    use super::package::TargetInstructionContract;
    for observation in ["frame", "missing region", "missing file"] {
        let root = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
        let mut locked = LockedInstall::acquire(&identity).unwrap();
        let agents = root.path().join("AGENTS.md");
        fs::write(&agents, "User prefix\nAudited target core\n").unwrap();
        locked
            .install_package(
                super::package_tests::package(root.path(), "old"),
                &InstallVersion::current(),
                &mut NoStamp,
            )
            .unwrap();
        match observation {
            "frame" => {
                let mut bytes = fs::read(&agents).unwrap();
                bytes.extend_from_slice(b"User suffix\n");
                fs::write(&agents, bytes).unwrap();
            }
            "missing region" => fs::write(&agents, "Current surviving user text").unwrap(),
            "missing file" => fs::remove_file(&agents).unwrap(),
            _ => unreachable!(),
        }
        let after = match observation {
            "frame" => {
                b"User prefix\nAudited target core\nAudited target core\nUser suffix\n".to_vec()
            }
            "missing region" => b"Current surviving user text\nAudited target core\n".to_vec(),
            _ => b"Audited target core\n".to_vec(),
        };
        let down = LegacyWorkspacePackage {
            skills: rendered("legacy"),
            instructions: Some(InstructionRestore {
                before: fs::read(&agents).ok(),
                after: Some(after.clone()),
                target_contract: Some(TargetInstructionContract {
                    binary_version: "0.105.3".into(),
                    bytes: b"Audited target core\n".to_vec(),
                }),
            }),
            config_patches: Vec::new(),
            source_patches: Vec::new(),
        };
        let version = InstallVersion {
            binary_version: "0.105.3".into(),
            compatibility_revision: 1,
        };
        locked.preflight_legacy_package(&down, &version).unwrap();
        locked
            .install_legacy_package(down, &version, &mut NoStamp)
            .unwrap();
        assert_eq!(fs::read(agents).unwrap(), after);
    }
}
