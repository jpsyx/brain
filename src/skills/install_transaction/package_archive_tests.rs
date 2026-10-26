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
