use brain::{
    access::{AccessMode, MachineCapabilityEnvironment, capability_plan},
    config::Config,
};
use std::fs;

use crate::support::{actor, family_id, temporary_workspace};

#[test]
fn selected_renderer_rejects_invalid_and_ambiguous_customization_before_output() {
    for fault in ["invalid", "duplicate", "symlink", "unreadable"] {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let (_home, workspace) = temporary_workspace();
        let extensions = workspace.root().join(".config/extensions");
        fs::create_dir_all(&extensions).unwrap();
        let source = extensions.join("todo.md");
        match fault {
            "invalid" => fs::write(&source, b"\xff").unwrap(),
            "duplicate" => fs::write(&source, "[todo:linear]\nA\n[todo:linear]\nB\n").unwrap(),
            "symlink" => {
                fs::write(extensions.join("target"), "CUSTOM").unwrap();
                symlink(extensions.join("target"), &source).unwrap();
            }
            _ => {
                fs::write(&source, "CUSTOM").unwrap();
                fs::set_permissions(&source, fs::Permissions::from_mode(0o000)).unwrap();
            }
        }
        let config = Config {
            access_mode: AccessMode::WorkspaceOnly,
            allowed_skills: vec!["todo".to_owned()],
            ..Config::default()
        };
        let machine =
            MachineCapabilityEnvironment::from_value(family_id(), serde_json::json!({})).unwrap();
        let plan = capability_plan(&config, &machine).unwrap();
        let selected = actor();
        assert!(
            brain::skills::render_workspace_capabilities(&workspace, &selected, &plan).is_err(),
            "{fault}"
        );
        assert!(
            !workspace
                .paths()
                .capability_skills_dir(selected.user_id())
                .join("todo")
                .exists()
        );
        assert!(fs::symlink_metadata(source).is_ok());
    }
}

#[test]
fn exact_selected_machine_source_disappearing_after_resolution_is_an_error() {
    let (_home, workspace) = temporary_workspace();
    let source = workspace.root().join("machine/custom");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("SKILL.md"), "# custom\n").unwrap();
    let config = Config {
        access_mode: AccessMode::WorkspaceOnly,
        allowed_skills: vec!["custom".to_owned()],
        ..Config::default()
    };
    let machine = MachineCapabilityEnvironment::from_value(
        family_id(),
        serde_json::json!({
            "skills": [{"name": "custom", "path": source}]
        }),
    )
    .unwrap();
    let plan = capability_plan(&config, &machine).unwrap();
    assert_eq!(plan.skills.available_names(), ["custom"]);
    fs::remove_file(source.join("SKILL.md")).unwrap();
    assert!(brain::skills::render_workspace_capabilities(&workspace, &actor(), &plan).is_err());
}
