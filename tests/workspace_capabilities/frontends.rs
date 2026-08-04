use std::path::PathBuf;
use std::sync::Arc;

use brain::access::{
    AccessMode, CapabilityEnforcement, MachineCapabilityEnvironment, capability_plan,
};
use brain::agent::{
    AgentFrontend, AgentSession, ClaudeFrontend, CodexFrontend, LaunchRequest, SessionPlan,
};
use brain::config::Config;

use crate::support::{actor, family_id, temporary_workspace};

#[cfg(unix)]
#[test]
fn claude_uses_owner_only_workspace_mcp_json_and_strict_selection_flags() {
    use std::os::unix::fs::PermissionsExt;

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
    let frontend = ClaudeFrontend::new(
        "claude",
        workspace.root().to_path_buf(),
        PathBuf::from("/unused/projects"),
    );

    let spec = frontend.launch_spec(&request).expect("Claude launch spec");

    let config_path = workspace.paths().capability_mcp_config();
    assert!(spec.command.contains("--mcp-config"), "{}", spec.command);
    assert!(
        spec.command.contains("--strict-mcp-config"),
        "{}",
        spec.command
    );
    assert!(!spec.command.contains("--bare"), "{}", spec.command);
    assert!(spec.command.contains(&config_path.display().to_string()));
    assert!(!spec.command.contains("machine-secret"));
    let runtime: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&config_path).expect("workspace runtime MCP JSON"))
            .expect("runtime MCP JSON");
    assert_eq!(
        runtime["mcpServers"]["notion"]["url"],
        "https://notion.example.test/mcp"
    );
    assert_eq!(
        runtime["mcpServers"]["notion"]["headers"]["Authorization"],
        "Bearer machine-secret"
    );
    assert!(runtime["mcpServers"].get("linear").is_none());
    assert_eq!(
        std::fs::metadata(&config_path)
            .expect("runtime MCP metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(config_path.parent().expect("capability directory"))
            .expect("capability directory metadata")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        spec.capabilities.mcps.enforcement("notion"),
        Some(CapabilityEnforcement::StrictlySelected)
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

    let spec = CodexFrontend::new("codex")
        .launch_spec(&request)
        .expect("Codex launch spec");

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
        CodexFrontend::new("codex")
            .launch_spec(&request)
            .expect("Codex launch spec")
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
    let spec = CodexFrontend::new("codex")
        .launch_spec(&request)
        .expect("Codex launch spec");
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
