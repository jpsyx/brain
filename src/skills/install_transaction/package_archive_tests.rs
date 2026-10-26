use super::{
    InstallIdentity, InstallVersion, LockedInstall, NoStamp,
    package::{InstructionRestore, LegacyWorkspacePackage},
    package_tests::package,
};
use std::fs;

#[test]
fn retained_package_reader_returns_typed_candidates_and_rejects_tampering_links_and_foreign_identity()
 {
    for conflict in ["none", "corrupt", "symlink", "root", "uuid", "original"] {
        let root = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
        let mut locked = LockedInstall::acquire(&identity).unwrap();
        locked
            .install_package(
                package(root.path(), "old"),
                &InstallVersion::current(),
                &mut NoStamp,
            )
            .unwrap();
        let down = LegacyWorkspacePackage {
            skills: crate::skills::transaction_tests::rendered("legacy"),
            instructions: Some(InstructionRestore {
                target_contract: None,
                before: Some(fs::read(root.path().join("AGENTS.md")).unwrap()),
                after: None,
            }),
            config_patches: Vec::new(),
            source_patches: Vec::new(),
        };
        locked
            .install_legacy_package(
                down,
                &InstallVersion {
                    binary_version: "0.105.3".into(),
                    compatibility_revision: 1,
                },
                &mut NoStamp,
            )
            .unwrap();
        let directory = cache.path().join("skills-install/package-backups");
        let file = fs::read_dir(&directory)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        match conflict {
            "corrupt" => fs::write(&file, b"changed archive").unwrap(),
            "symlink" => {
                let outside = cache.path().join("outside.json");
                fs::rename(&file, &outside).unwrap();
                std::os::unix::fs::symlink(outside, &file).unwrap();
            }
            "root" | "uuid" | "original" => {
                let mut value: serde_json::Value =
                    serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
                if conflict == "root" {
                    value["root"] = "/unrelated/workspace".into();
                } else if conflict == "original" {
                    value["package"]["instructions"]["original"] =
                        serde_json::json!([102, 111, 114, 103, 101, 100]);
                } else {
                    value["workspace_id"] = "00000000-0000-4000-8000-000000000000".into();
                }
                let bytes = serde_json::to_vec(&value).unwrap();
                let replacement = directory.join(format!(
                    "{}-{}.json",
                    value["transaction_id"].as_str().unwrap(),
                    super::snapshot::digest(&bytes)
                ));
                fs::write(replacement, bytes).unwrap();
                fs::remove_file(&file).unwrap();
            }
            _ => {}
        }
        let result = locked.retained_packages();
        if conflict == "none" {
            let candidates = result.unwrap();
            assert_eq!(candidates.len(), 1);
            let candidate = &candidates[0];
            assert_eq!(candidate.target.binary_version, "0.105.3");
            assert_eq!(candidate.previous.as_ref().unwrap().schema_version, 2);
            assert!(candidate.metadata.instructions.is_some());
            assert_eq!(
                candidate
                    .metadata
                    .extension_snapshot
                    .as_ref()
                    .unwrap()
                    .checked()
                    .unwrap()
                    .projection
                    .sources[".config/extensions/custom.md"],
                b"old"
            );
            assert_eq!(candidate.artifacts["AGENTS.md"].after, None);
            assert!(uuid::Uuid::parse_str(&candidate.transaction_id).is_ok());
        } else {
            assert!(result.is_err(), "{conflict}");
        }
    }
}

#[test]
fn retained_applicability_requires_matching_postimages_but_ignores_unrelated_edits() {
    use super::package::{JsonFieldPatch, TargetInstructionContract, compose_target_instructions};
    use std::collections::BTreeMap;
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(".config/extensions")).unwrap();
    fs::write(
        root.path().join(".config/config.json"),
        br#"{"allowed_skills":["todo"]}"#,
    )
    .unwrap();
    let canonical = ".config/extensions/agenda.md";
    let canonical_bytes = b"[agenda:anchors]\nANCHOR\n";
    fs::write(root.path().join(canonical), canonical_bytes).unwrap();
    let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
    let mut locked = LockedInstall::acquire(&identity).unwrap();
    let mut modern = package(root.path(), "old");
    modern.config_patches.push(JsonFieldPatch {
        relative_path: ".config/config.json".into(),
        field: "allowed_skills".into(),
        before: Some(serde_json::json!(["todo"])),
        after: Some(serde_json::json!(["todo", "agenda"])),
    });
    locked
        .install_package(modern, &InstallVersion::current(), &mut NoStamp)
        .unwrap();
    let manifest = locked.manifest().unwrap().unwrap();
    let owned = manifest
        .package
        .as_ref()
        .unwrap()
        .instructions
        .as_ref()
        .unwrap();
    let contract = TargetInstructionContract {
        binary_version: "0.105.3".into(),
        bytes: b"Target core\n".to_vec(),
    };
    let before = fs::read(root.path().join("AGENTS.md")).unwrap();
    let after = compose_target_instructions(Some(&before), owned, &contract).unwrap();
    let projection = crate::skills::compatibility::project_extensions(
        &BTreeMap::from([(
            canonical.to_owned(),
            String::from_utf8(canonical_bytes.to_vec()).unwrap(),
        )]),
        crate::skills::compatibility::ProjectionDirection::Legacy105,
    )
    .unwrap();
    let legacy_source = projection.legacy_source_patches[0].after.clone();
    let down = LegacyWorkspacePackage {
        skills: crate::skills::transaction_tests::rendered("legacy"),
        instructions: Some(InstructionRestore {
            target_contract: Some(contract),
            before: Some(before.clone()),
            after: Some(after.clone()),
        }),
        config_patches: vec![JsonFieldPatch {
            relative_path: ".config/config.json".into(),
            field: "allowed_skills".into(),
            before: Some(serde_json::json!(["todo", "agenda"])),
            after: Some(serde_json::json!(["todo"])),
        }],
        source_patches: projection.legacy_source_patches,
    };
    let target = InstallVersion {
        binary_version: "0.105.3".into(),
        compatibility_revision: 1,
    };
    assert!(
        locked
            .install_legacy_package_with(down.clone(), &target, &mut NoStamp, &mut |point| {
                anyhow::ensure!(
                    point != super::Checkpoint::Staged,
                    "abort before publication"
                );
                Ok(())
            })
            .is_err()
    );
    let candidates = locked.retained_packages().unwrap();
    let candidate = &candidates[0];
    let current = BTreeMap::from([
        ("AGENTS.md".to_owned(), before),
        (
            ".config/config.json".to_owned(),
            fs::read(root.path().join(".config/config.json")).unwrap(),
        ),
        (canonical.to_owned(), canonical_bytes.to_vec()),
    ]);
    assert!(!candidate.applies_to(&manifest, &current).unwrap());
    locked
        .install_legacy_package(down, &target, &mut NoStamp)
        .unwrap();
    let mut target_manifest = locked.manifest().unwrap().unwrap();
    target_manifest
        .files
        .insert(".agents/skills/extra/SKILL.md".into(), "a".repeat(64));
    let mut current = BTreeMap::from([
        (
            "AGENTS.md".into(),
            [b"Later prefix\n".as_slice(), &after, b"Later suffix\n"].concat(),
        ),
        (
            ".config/config.json".into(),
            br#"{"allowed_skills":["todo"],"agenda_dir":"later"}"#.to_vec(),
        ),
        (canonical.into(), canonical_bytes.to_vec()),
        (".config/extensions/todo.md".into(), legacy_source),
    ]);
    assert!(candidate.applies_to(&target_manifest, &current).unwrap());
    current.insert(
        ".config/extensions/todo.md".into(),
        b"[todo:anchors]\nOLD CLIENT EDIT\n".to_vec(),
    );
    assert!(!candidate.applies_to(&target_manifest, &current).unwrap());
    current.insert(
        ".config/extensions/todo.md".into(),
        candidate.artifacts[".config/extensions/todo.md"]
            .after
            .clone()
            .unwrap(),
    );
    current.insert(
        ".config/config.json".into(),
        br#"{"allowed_skills":["agenda"]}"#.to_vec(),
    );
    assert!(!candidate.applies_to(&target_manifest, &current).unwrap());
}
