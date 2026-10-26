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

#[test]
fn actual_agenda_only_capability_matches_ordinary_legacy_projection() {
    let (_home, workspace) = temporary_workspace();
    let extensions = workspace.root().join(".config/extensions");
    fs::create_dir_all(&extensions).unwrap();
    let source = "MIXED task and scheduling preferences\n[todo:anchors]\nLEGACY ANCHOR\n[todo:unknown]\nUNKNOWN\n";
    fs::write(extensions.join("todo.md"), source).unwrap();
    let sources = brain::skills::real_sources(&workspace);
    brain::skills::install::sync(
        &brain::skills::layout::Layout::under_root(workspace.root()),
        &sources,
    )
    .unwrap();
    let config = Config {
        access_mode: AccessMode::WorkspaceOnly,
        allowed_skills: vec!["agenda".to_owned()],
        ..Config::default()
    };
    let machine =
        MachineCapabilityEnvironment::from_value(family_id(), serde_json::json!({})).unwrap();
    let plan = capability_plan(&config, &machine).unwrap();
    assert_eq!(plan.skills.available_names(), ["agenda"]);
    let report = brain::skills::render_workspace_capabilities(&workspace, &actor(), &plan).unwrap();
    assert_eq!(report.rendered, ["agenda"]);
    let ordinary = workspace.root().join(".agents/skills/agenda");
    let selected = report.rendered_dir.join("agenda");
    for path in ["SKILL.md", "references/legacy-task-context.md"] {
        assert_eq!(
            fs::read(ordinary.join(path)).unwrap(),
            fs::read(selected.join(path)).unwrap()
        );
    }
    let agenda = fs::read_to_string(selected.join("SKILL.md")).unwrap();
    assert_eq!(agenda.matches("LEGACY ANCHOR").count(), 1);
    assert!(agenda.contains("read its `AGENTS.md`"));
    let legacy = fs::read_to_string(selected.join("references/legacy-task-context.md")).unwrap();
    for required in [
        "MIXED",
        "UNKNOWN",
        "sha256:",
        "Do not rerun completed side effects",
    ] {
        assert!(legacy.contains(required));
    }
    assert_eq!(
        fs::read(extensions.join("todo.md")).unwrap(),
        source.as_bytes()
    );
}
