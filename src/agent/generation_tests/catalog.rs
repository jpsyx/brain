use super::*;

#[test]
fn a_second_launch_never_replaces_the_first_launchs_referenced_skill_generation() {
    use crate::agent::{AgentFrontend, PiFrontend};
    let home = tempfile::tempdir().unwrap();
    let workspace = workspace(home.path());
    let frontend = PiFrontend::for_workspace("printf '%s\\0'", workspace.root());
    let selected_path = |request: &LaunchRequest| {
        let spec = frontend.launch_spec(request).unwrap();
        let output = std::process::Command::new("sh")
            .args(["-c", &spec.command])
            .output()
            .unwrap();
        assert!(output.status.success());
        let arguments = output.stdout.split(|byte| *byte == 0).collect::<Vec<_>>();
        let index = arguments
            .iter()
            .position(|argument| *argument == b"--skill")
            .unwrap();
        std::path::PathBuf::from(std::str::from_utf8(arguments[index + 1]).unwrap())
    };
    extensions(&workspace, "OLD");
    crate::skills::install::sync_workspace(&workspace, &crate::skills::real_sources(&workspace))
        .unwrap();
    let old_request = request(&workspace).prepare_instructions().unwrap();
    let old_path = selected_path(&old_request);
    let old_file = old_path.join("custom/SKILL.md");
    let old_modified = fs::metadata(&old_file).unwrap().modified().unwrap();
    extensions(&workspace, "NEW");
    crate::skills::install::sync_workspace(&workspace, &crate::skills::real_sources(&workspace))
        .unwrap();
    let new_request = request(&workspace).prepare_instructions().unwrap();
    let new_path = selected_path(&new_request);
    assert!(
        fs::read_to_string(&old_file)
            .unwrap()
            .contains("OLD custom")
    );
    assert!(
        fs::read_to_string(new_path.join("custom/SKILL.md"))
            .unwrap()
            .contains("NEW custom")
    );
    assert_ne!(old_path, new_path);
    assert_eq!(selected_path(&old_request), old_path);
    assert_eq!(
        fs::metadata(old_file).unwrap().modified().unwrap(),
        old_modified
    );
}

#[test]
fn claude_and_codex_receive_the_captured_catalog_with_only_a_read_exception() {
    use crate::agent::{AgentFrontend, ClaudeFrontend, CodexFrontend};
    use std::os::unix::fs::PermissionsExt as _;
    let home = tempfile::tempdir().unwrap();
    let workspace = workspace(home.path());
    extensions(&workspace, "OLD");
    let prepared = request(&workspace).prepare_instructions().unwrap();
    let rendered = prepared
        .render_capabilities(prepared.access_policy().capability_plan().unwrap())
        .unwrap();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let codex = home.path().join("codex-fixture");
    let script = fs::read_to_string(fixture.join("codex/fake_codex.sh"))
        .unwrap()
        .replace("Harness fixture", "Existing global rules")
        .replace("permissions.instructions", "generic.developer_instructions")
        .replace(
            r#"{"type":"harness"}"#,
            r#"{"type":"developer_instructions","from_additional_requirements":false}"#,
        );
    fs::write(&codex, script).unwrap();
    fs::set_permissions(&codex, fs::Permissions::from_mode(0o700)).unwrap();
    let frontends: [Box<dyn AgentFrontend>; 2] = [
        Box::new(ClaudeFrontend::new(
            format!(
                "{} --append-system-prompt 'Existing global rules'",
                fixture.join("claude/fake_claude.sh").display()
            ),
            home.path().into(),
            home.path().join("projects"),
        )),
        Box::new(CodexFrontend::new(codex.display().to_string())),
    ];
    for frontend in frontends {
        let spec = frontend.launch_spec(&prepared).unwrap();
        let output = std::process::Command::new("sh")
            .args(["-c", &spec.command])
            .current_dir(&spec.cwd)
            .output()
            .unwrap();
        assert!(output.status.success());
        let arguments = output
            .stdout
            .split(|byte| *byte == 0)
            .map(|bytes| String::from_utf8(bytes.to_vec()).unwrap())
            .collect::<Vec<_>>();
        let delivered = arguments
            .iter()
            .find_map(|argument| argument.strip_prefix("developer_instructions="))
            .map_or_else(
                || {
                    arguments
                        .windows(2)
                        .rfind(|pair| pair[0] == "--append-system-prompt")
                        .unwrap()[1]
                        .clone()
                },
                |value| serde_json::from_str::<String>(value).unwrap(),
            );
        assert!(
            delivered.contains(&rendered.rendered_dir.display().to_string()),
            "captured catalog missing from decoded instructions"
        );
        for phrase in [
            "Existing global rules",
            "authoritative definitions",
            "read-only exception",
            "custom/SKILL.md",
            "agenda/SKILL.md",
            "no writes or execution",
            "sibling caches",
            "original external sources",
            "MCPs",
            "recipients",
        ] {
            assert!(delivered.contains(phrase), "missing {phrase}: {delivered}");
        }
        assert!(
            prepared
                .instructions()
                .unwrap()
                .trusted_policy()
                .contains("read-only exception")
        );
    }
}

#[test]
fn empty_selection_has_no_read_exception_and_retained_conflicts_are_not_overwritten() {
    let home = tempfile::tempdir().unwrap();
    let workspace = workspace(home.path());
    let empty = capability_plan(
        &Config {
            access_mode: AccessMode::WorkspaceOnly,
            allowed_skills: vec![],
            ..Config::default()
        },
        &MachineCapabilityEnvironment::from_value(workspace.id(), serde_json::json!({})).unwrap(),
    )
    .unwrap();
    let prepared = request(&workspace)
        .with_capability_plan(empty)
        .prepare_instructions()
        .unwrap();
    assert!(
        !prepared
            .instructions()
            .unwrap()
            .trusted_policy()
            .contains("read-only exception")
    );
    let selected = request(&workspace).prepare_instructions().unwrap();
    let report = selected
        .render_capabilities(selected.access_policy().capability_plan().unwrap())
        .unwrap();
    let file = report.rendered_dir.join("custom/SKILL.md");
    fs::write(&file, "changed cache bytes").unwrap();
    assert!(request(&workspace).prepare_instructions().is_err());
    assert_eq!(fs::read_to_string(file).unwrap(), "changed cache bytes");
}
