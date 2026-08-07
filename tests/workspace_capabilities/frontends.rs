use std::sync::Arc;

use brain::access::{
    AccessMode, CapabilityEnforcement, MachineCapabilityEnvironment, capability_plan,
};
use brain::agent::{AgentKind, AgentSession, LaunchRequest, SessionPlan};
use brain::config::Config;

use crate::support::{actor, family_id, launch_spec, temporary_workspace};

#[test]
fn codex_uses_documented_secret_free_overrides_and_reports_base_exclusion_as_advisory() {
    let (_home, workspace) = temporary_workspace();
    let config = Config {
        access_mode: AccessMode::WorkspaceOnly,
        allowed_mcps: vec!["notion".to_owned()],
        ..Config::default()
    };
    let machine = MachineCapabilityEnvironment::from_value(
        family_id(),
        serde_json::json!({
            "mcps": [
                {
                    "name": "notion",
                    "url": "https://notion.example.test/mcp",
                    "credentials": {
                        "headers": {"Authorization": "Bearer machine-secret"}
                    }
                },
                {"name": "linear", "command": "/opt/local/bin/linear-mcp"}
            ]
        }),
    )
    .expect("machine capability environment");
    let plan = capability_plan(&config, &machine).expect("capability plan");
    let launch_actor = actor();
    let request = LaunchRequest::from_trusted_context(
        Arc::clone(&workspace),
        launch_actor.clone(),
        SessionPlan::fresh(AgentSession::new("session-1").expect("session")),
        None,
        AccessMode::WorkspaceOnly,
    )
    .with_capability_plan(plan);

    let spec = launch_spec(AgentKind::Codex, "codex", &request).expect("Codex launch spec");

    let isolated_name = "mcp_servers.brain_ws_8ccd7c411b6e4a3cb91e1b0117b77a2b_6e6f74696f6e";
    assert!(spec.command.contains(&format!("{isolated_name}.url=")));
    assert!(spec.command.contains("https://notion.example.test/mcp"));
    assert!(
        spec.command
            .contains(&format!("{isolated_name}.enabled=true"))
    );
    assert!(
        spec.command
            .contains(&format!("{isolated_name}.env_http_headers="))
    );
    assert!(!spec.command.contains("mcp_servers.notion."));
    assert!(!spec.command.contains("machine-secret"));
    assert!(!spec.command.contains("mcp_servers.linear"));
    assert!(!spec.command.contains("--profile"));
    assert!(
        spec.environment
            .iter()
            .any(|(_, value)| value == "Bearer machine-secret")
    );
    assert!(
        spec.environment
            .iter()
            .all(|(name, _)| name != "CODEX_HOME")
    );
    assert_eq!(
        spec.capabilities.mcps.enforcement("notion"),
        Some(CapabilityEnforcement::AdvisoryOnly)
    );
    assert_eq!(
        spec.capabilities.skills.enforcement("todo"),
        Some(CapabilityEnforcement::AdvisoryOnly)
    );
    assert!(
        workspace
            .paths()
            .capability_skills_dir(launch_actor.user_id())
            .join("todo/SKILL.md")
            .is_file()
    );
}

#[test]
fn codex_workspace_server_keys_are_stable_and_collision_free_for_punctuation() {
    let (_home, workspace) = temporary_workspace();
    let config = Config {
        access_mode: AccessMode::WorkspaceOnly,
        allowed_mcps: vec!["foo-bar".to_owned(), "foo_bar".to_owned()],
        allowed_skills: Vec::new(),
        ..Config::default()
    };
    let machine = MachineCapabilityEnvironment::from_value(
        family_id(),
        serde_json::json!({
            "mcps": [
                {
                    "name": "foo-bar",
                    "url": "https://one.example.test/mcp",
                    "credentials": {"headers": {"Authorization": "secret-one"}}
                },
                {
                    "name": "foo_bar",
                    "url": "https://two.example.test/mcp",
                    "credentials": {"headers": {"Authorization": "secret-two"}}
                }
            ]
        }),
    )
    .expect("machine capability environment");
    let launch = || {
        let plan = capability_plan(&config, &machine).expect("capability plan");
        let request = LaunchRequest::from_trusted_context(
            Arc::clone(&workspace),
            actor(),
            SessionPlan::fresh(AgentSession::new("session-1").expect("session")),
            None,
            AccessMode::WorkspaceOnly,
        )
        .with_capability_plan(plan);
        launch_spec(AgentKind::Codex, "codex", &request).expect("Codex launch spec")
    };

    let first = launch();
    let second = launch();

    assert_eq!(first.command, second.command);
    assert_eq!(first.environment, second.environment);
    assert!(
        first
            .command
            .contains("mcp_servers.brain_ws_8ccd7c411b6e4a3cb91e1b0117b77a2b_666f6f2d626172.url=")
    );
    assert!(
        first
            .command
            .contains("mcp_servers.brain_ws_8ccd7c411b6e4a3cb91e1b0117b77a2b_666f6f5f626172.url=")
    );
    let secret_names = first
        .environment
        .iter()
        .filter(|(_, value)| matches!(value.as_str(), "secret-one" | "secret-two"))
        .map(|(name, _)| name)
        .collect::<Vec<_>>();
    assert_eq!(secret_names.len(), 2);
    assert_ne!(secret_names[0], secret_names[1]);
}

#[cfg(unix)]
#[test]
fn codex_remaps_same_named_stdio_secrets_into_collision_free_mcp_child_environments() {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let (_home, workspace) = temporary_workspace();
    let capture = workspace.root().join("capture-mcp-env");
    std::fs::write(&capture, "#!/bin/sh\nprintf '%s' \"$TOKEN\" > \"$1\"\n").expect("capture MCP");
    std::fs::set_permissions(&capture, std::fs::Permissions::from_mode(0o700))
        .expect("capture MCP permissions");
    let first_output = workspace.root().join("first-token");
    let second_output = workspace.root().join("second-token");
    let config = Config {
        access_mode: AccessMode::WorkspaceOnly,
        allowed_mcps: vec!["first".to_owned(), "second".to_owned()],
        allowed_skills: Vec::new(),
        ..Config::default()
    };
    let machine = MachineCapabilityEnvironment::from_value(
        family_id(),
        serde_json::json!({
            "mcps": [
                {
                    "name": "first",
                    "command": capture,
                    "args": [first_output],
                    "credentials": {"environment": {"TOKEN": "first-secret"}}
                },
                {
                    "name": "second",
                    "command": capture,
                    "args": [second_output],
                    "credentials": {"environment": {"TOKEN": "second-secret"}}
                }
            ]
        }),
    )
    .expect("machine capability environment");
    let plan = capability_plan(&config, &machine).expect("capability plan");
    let request = LaunchRequest::from_trusted_context(
        Arc::clone(&workspace),
        actor(),
        SessionPlan::fresh(AgentSession::new("session-1").expect("session")),
        None,
        AccessMode::WorkspaceOnly,
    )
    .with_capability_plan(plan);

    let spec = launch_spec(AgentKind::Codex, "codex", &request).expect("Codex launch spec");

    let secret_entries = spec
        .environment
        .iter()
        .filter(|(_, value)| matches!(value.as_str(), "first-secret" | "second-secret"))
        .collect::<Vec<_>>();
    assert_eq!(secret_entries.len(), 2);
    assert_ne!(secret_entries[0].0, secret_entries[1].0);
    assert!(
        secret_entries
            .iter()
            .all(|(name, _)| name.starts_with("BRAIN_MCP_"))
    );
    assert!(spec.environment.iter().all(|(name, _)| name != "TOKEN"));

    let wrappers = std::fs::read_dir(workspace.paths().capabilities_dir().join("codex-mcp"))
        .expect("Codex MCP wrapper directory")
        .map(|entry| entry.expect("wrapper entry").path())
        .collect::<Vec<_>>();
    assert_eq!(wrappers.len(), 2);
    for wrapper in wrappers {
        let contents = std::fs::read_to_string(&wrapper).expect("wrapper contents");
        assert!(!contents.contains("first-secret"));
        assert!(!contents.contains("second-secret"));
        let output = Command::new(&wrapper)
            .env_clear()
            .envs(spec.environment.iter().cloned())
            .output()
            .expect("run generated MCP wrapper");
        assert!(output.status.success());
    }
    assert_eq!(
        std::fs::read_to_string(first_output).expect("first child token"),
        "first-secret"
    );
    assert_eq!(
        std::fs::read_to_string(second_output).expect("second child token"),
        "second-secret"
    );
}

#[test]
fn installed_codex_parser_accepts_the_generated_per_invocation_overrides() {
    use std::process::Command;

    if Command::new("codex").arg("--version").output().is_err() {
        return;
    }
    let (_home, workspace) = temporary_workspace();
    let config = Config {
        access_mode: AccessMode::WorkspaceOnly,
        allowed_mcps: vec!["notion".to_owned()],
        ..Config::default()
    };
    let machine = MachineCapabilityEnvironment::from_value(
        family_id(),
        serde_json::json!({
            "mcps": [{
                "name": "notion",
                "url": "https://notion.example.test/mcp",
                "credentials": {"headers": {"Authorization": "Bearer parser-secret"}}
            }]
        }),
    )
    .expect("machine capability environment");
    let plan = capability_plan(&config, &machine).expect("capability plan");
    let request = LaunchRequest::from_trusted_context(
        Arc::clone(&workspace),
        actor(),
        SessionPlan::fresh(AgentSession::new("session-1").expect("session")),
        None,
        AccessMode::WorkspaceOnly,
    )
    .with_capability_plan(plan);
    let spec = launch_spec(AgentKind::Codex, "codex", &request).expect("Codex launch spec");
    let parser_command = format!(
        "{} -- 'capability parser probe' >/dev/null",
        spec.command
            .replacen("codex", "codex debug prompt-input", 1)
    );

    let output = Command::new("/bin/sh")
        .args(["-c", &parser_command])
        .envs(spec.environment)
        .output()
        .expect("run installed Codex parser");

    assert!(
        output.status.success(),
        "installed Codex rejected generated -c overrides: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
