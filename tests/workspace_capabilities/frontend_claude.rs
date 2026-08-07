use std::sync::Arc;

use brain::access::{
    AccessMode, CapabilityEnforcement, MachineCapabilityEnvironment, capability_plan,
};
use brain::agent::{AgentKind, AgentSession, LaunchRequest, SessionPlan};
use brain::config::Config;

use crate::support::{actor, family_id, launch_spec, temporary_workspace};

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
    let spec = launch_spec(AgentKind::Claude, "claude", &request).expect("Claude launch spec");

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
fn claude_downgrades_strict_mcp_claims_for_ambiguous_or_indirect_commands() {
    let (_home, workspace) = temporary_workspace();
    let config = Config {
        access_mode: AccessMode::WorkspaceOnly,
        allowed_mcps: vec!["notion".to_owned()],
        allowed_skills: Vec::new(),
        ..Config::default()
    };
    let machine = MachineCapabilityEnvironment::from_value(
        family_id(),
        serde_json::json!({
            "mcps": [{"name": "notion", "url": "https://notion.example.test/mcp"}]
        }),
    )
    .expect("machine capability environment");

    for command in [
        "claude; printf bypass",
        "claude # appended flags are ignored",
        "sh -c 'exec claude'",
        "claude -- --strict-mcp-config",
    ] {
        let plan = capability_plan(&config, &machine).expect("capability plan");
        let request = LaunchRequest::from_trusted_context(
            Arc::clone(&workspace),
            actor(),
            SessionPlan::fresh(AgentSession::new("session-1").expect("session")),
            None,
            AccessMode::WorkspaceOnly,
        )
        .with_capability_plan(plan);

        let spec = launch_spec(AgentKind::Claude, command, &request).expect("Claude launch spec");

        assert_eq!(
            spec.capabilities.mcps.enforcement("notion"),
            Some(CapabilityEnforcement::AdvisoryOnly),
            "configured command: {command}"
        );
    }
}
