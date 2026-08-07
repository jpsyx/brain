use std::sync::Arc;

use brain::access::{
    AccessMode, CapabilityEnforcement, MachineCapabilityEnvironment, capability_plan,
};
use brain::agent::{AgentKind, AgentSession, LaunchRequest, SessionPlan};
use brain::config::Config;

use crate::support::{actor, family_id, launch_spec, temporary_workspace};

#[test]
fn merges_selected_mcps_and_actor_skills_without_serializing_secrets() {
    let (_home, workspace) = temporary_workspace();
    let config = Config {
        access_mode: AccessMode::WorkspaceOnly,
        allowed_mcps: vec!["local".to_owned(), "remote".to_owned()],
        allowed_skills: vec!["todo".to_owned()],
        ..Config::default()
    };
    let machine = MachineCapabilityEnvironment::from_value(
        family_id(),
        serde_json::json!({
            "mcps": [
                {
                    "name": "local",
                    "command": "/opt/local/bin/mcp",
                    "args": ["--stdio"],
                    "credentials": {"environment": {"TOKEN": "local-secret"}}
                },
                {
                    "name": "remote",
                    "url": "https://remote.example.test/mcp",
                    "credentials": {
                        "headers": {"X-Workspace": "header-secret"},
                        "bearer_token": "bearer-secret"
                    }
                },
                {"name": "excluded", "command": "/opt/local/bin/excluded"}
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

    let spec =
        launch_spec(AgentKind::OpenCode, "opencode", &request).expect("OpenCode launch spec");
    let raw = spec
        .environment
        .iter()
        .find(|(name, _)| name == "OPENCODE_CONFIG_CONTENT")
        .map(|(_, value)| value)
        .expect("inline OpenCode config");
    let value: serde_json::Value = serde_json::from_str(raw).expect("valid inline config");
    let prefix = "brain_ws_8ccd7c411b6e4a3cb91e1b0117b77a2b_";
    let local = &value["mcp"][format!("{prefix}6c6f63616c")];
    let remote = &value["mcp"][format!("{prefix}72656d6f7465")];

    assert_eq!(local["type"], "local");
    assert_eq!(
        local["command"],
        serde_json::json!(["/opt/local/bin/mcp", "--stdio"])
    );
    assert!(
        local["environment"]["TOKEN"]
            .as_str()
            .is_some_and(|value| value.starts_with("{env:BRAIN_MCP_"))
    );
    assert_eq!(remote["type"], "remote");
    assert_eq!(remote["url"], "https://remote.example.test/mcp");
    assert!(
        remote["headers"]["X-Workspace"]
            .as_str()
            .is_some_and(|value| value.starts_with("{env:BRAIN_MCP_"))
    );
    assert!(
        remote["headers"]["Authorization"]
            .as_str()
            .is_some_and(|value| value.starts_with("Bearer {env:BRAIN_MCP_"))
    );
    assert!(value["mcp"].get("excluded").is_none());
    assert!(!raw.contains("local-secret"));
    assert!(!raw.contains("header-secret"));
    assert!(!raw.contains("bearer-secret"));
    for secret in ["local-secret", "header-secret", "bearer-secret"] {
        assert!(spec.environment.iter().any(|(_, value)| value == secret));
    }
    assert_eq!(
        value["skills"]["paths"][0],
        workspace
            .paths()
            .capability_skills_dir(launch_actor.user_id())
            .display()
            .to_string()
    );
    assert_eq!(value["agent"]["brain"]["permission"]["skill"]["*"], "deny");
    assert_eq!(
        value["agent"]["brain"]["permission"]["skill"]["todo"],
        "allow"
    );
    assert!(
        workspace
            .paths()
            .capability_skills_dir(launch_actor.user_id())
            .join("todo/SKILL.md")
            .is_file()
    );
    assert_eq!(
        spec.capabilities.mcps.enforcement("local"),
        Some(CapabilityEnforcement::AdvisoryOnly)
    );
    assert_eq!(
        spec.capabilities.skills.enforcement("todo"),
        Some(CapabilityEnforcement::AdvisoryOnly)
    );
}

#[test]
fn omits_unavailable_and_excluded_mcps_from_the_reserved_config() {
    let (_home, workspace) = temporary_workspace();
    let plan = capability_plan(
        &Config {
            access_mode: AccessMode::WorkspaceOnly,
            allowed_mcps: vec!["missing-secret".to_owned()],
            allowed_skills: Vec::new(),
            ..Config::default()
        },
        &MachineCapabilityEnvironment::from_value(
            family_id(),
            serde_json::json!({
                "mcps": [
                    {
                        "name": "missing-secret",
                        "command": "/opt/local/bin/mcp",
                        "credentials": {"environment": {"TOKEN": null}}
                    },
                    {"name": "excluded", "command": "/opt/local/bin/excluded"}
                ]
            }),
        )
        .expect("machine capability environment"),
    )
    .expect("capability plan");
    let request = LaunchRequest::from_trusted_context(
        Arc::clone(&workspace),
        actor(),
        SessionPlan::fresh(AgentSession::new("session-1").expect("session")),
        None,
        AccessMode::WorkspaceOnly,
    )
    .with_capability_plan(plan);

    let spec =
        launch_spec(AgentKind::OpenCode, "opencode", &request).expect("OpenCode launch spec");
    let raw = spec
        .environment
        .iter()
        .find(|(name, _)| name == "OPENCODE_CONFIG_CONTENT")
        .map(|(_, value)| value)
        .expect("inline OpenCode config");
    let value: serde_json::Value = serde_json::from_str(raw).expect("valid inline config");

    assert!(
        value["mcp"]
            .as_object()
            .is_none_or(serde_json::Map::is_empty)
    );
    assert_eq!(
        spec.capabilities.mcps.enforcement("missing-secret"),
        Some(CapabilityEnforcement::Unavailable)
    );
    assert_eq!(spec.capabilities.mcps.enforcement("excluded"), None);
}
