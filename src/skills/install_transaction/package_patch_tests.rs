use super::{
    InstallIdentity, InstallVersion, LockedInstall, NoStamp,
    package::{JsonFieldPatch, ManagedWorkspacePackage},
};
use crate::skills::{
    compatibility::{ProjectionDirection, project_extensions},
    transaction_tests::rendered,
};
use std::{collections::BTreeMap, fs, path::Path};

pub(super) fn patched(root: &Path) -> ManagedWorkspacePackage {
    let canonical = ".config/extensions/agenda.md";
    let legacy = ".config/extensions/todo.md";
    let sources = BTreeMap::from([
        (
            canonical.into(),
            fs::read_to_string(root.join(canonical)).unwrap(),
        ),
        (
            legacy.into(),
            fs::read_to_string(root.join(legacy)).unwrap(),
        ),
    ]);
    ManagedWorkspacePackage {
        skills: rendered("new"),
        instructions: None,
        config_patches: vec![JsonFieldPatch {
            relative_path: ".config/config.json".into(),
            field: "allowed_skills".into(),
            before: Some(serde_json::json!(["second-brain"])),
            after: Some(serde_json::json!(["organize", "todo"])),
        }],
        source_patches: project_extensions(&sources, ProjectionDirection::Legacy105)
            .unwrap()
            .legacy_source_patches,
    }
}
pub(super) fn seed(root: &Path) {
    fs::create_dir_all(root.join(".config/extensions")).unwrap();
    fs::write(
        root.join(".config/config.json"),
        r#"{"allowed_skills":["second-brain"],"theme":"custom"}"#,
    )
    .unwrap();
    fs::write(
        root.join(".config/extensions/agenda.md"),
        "[agenda:anchors]\nCanonical anchor\n",
    )
    .unwrap();
    fs::write(
        root.join(".config/extensions/todo.md"),
        "Legacy user text\n",
    )
    .unwrap();
}

#[test]
fn config_field_and_legacy_sources_publish_with_skills_preserving_other_bytes_and_modes() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    seed(root.path());
    let config = root.path().join(".config/config.json");
    let source = root.path().join(".config/extensions/todo.md");
    fs::set_permissions(&config, fs::Permissions::from_mode(0o600)).unwrap();
    fs::set_permissions(&source, fs::Permissions::from_mode(0o640)).unwrap();
    let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
    let mut locked = LockedInstall::acquire(&identity).unwrap();
    let package = patched(root.path());
    locked
        .preflight_package(&package, &InstallVersion::current())
        .unwrap();
    locked
        .install_package(package, &InstallVersion::current(), &mut NoStamp)
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(&config).unwrap()).unwrap(),
        serde_json::json!({"allowed_skills":["organize","todo"],"theme":"custom"})
    );
    assert_eq!(
        fs::read(&source).unwrap(),
        b"Legacy user text\n\n[todo:anchors]\nCanonical anchor\n"
    );
    assert_eq!(
        fs::read(root.path().join(".config/extensions/agenda.md")).unwrap(),
        b"[agenda:anchors]\nCanonical anchor\n"
    );
    assert_eq!(
        fs::metadata(config).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(source).unwrap().permissions().mode() & 0o777,
        0o640
    );
}

#[test]
fn canonical_source_change_after_planning_blocks_publication_and_retains_evidence() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    seed(root.path());
    let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
    let mut locked = LockedInstall::acquire(&identity).unwrap();
    let canonical = root.path().join(".config/extensions/agenda.md");
    let result = locked.install_package_with(
        patched(root.path()),
        &InstallVersion::current(),
        &mut NoStamp,
        &mut |point| {
            if point == super::Checkpoint::Staged {
                fs::write(&canonical, "concurrent canonical edit\n")?;
            }
            Ok(())
        },
    );
    assert!(
        result.is_err(),
        "canonical provenance must be revalidated even if its legacy destination is unchanged"
    );
    assert_eq!(
        fs::read(root.path().join(".config/extensions/todo.md")).unwrap(),
        b"Legacy user text\n"
    );
    assert!(!root.path().join(".agents/skills/todo").exists());
    assert!(cache.path().join("skills-install/pending.json").is_file());
    assert!(locked.recover(&mut NoStamp).is_err());
    assert_eq!(fs::read(canonical).unwrap(), b"concurrent canonical edit\n");
}
