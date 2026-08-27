use std::sync::Arc;

use brain::access::{AccessMode, MachineCapabilityEnvironment, capability_plan};
use brain::agent::{AgentKind, AgentSession, LaunchRequest, SessionPlan};
use brain::config::Config;

use crate::support::{actor, family_id, launch_spec, temporary_workspace};

#[test]
fn debug_output_redacts_capability_credentials_prompts_and_launch_environment_values() {
    let (_home, workspace) = temporary_workspace();
    let machine = MachineCapabilityEnvironment::from_value(
        family_id(),
        serde_json::json!({
            "mcps": [{
                "name": "notion",
                "url": "https://private-endpoint.example.test/mcp",
                "credentials": {"bearer_token": "debug-capability-secret"}
            }]
        }),
    )
    .expect("machine capability environment");
    let plan = capability_plan(
        &Config {
            access_mode: AccessMode::WorkspaceOnly,
            allowed_mcps: vec!["notion".to_owned()],
            allowed_skills: Vec::new(),
            ..Config::default()
        },
        &machine,
    )
    .expect("capability plan");
    let request = LaunchRequest::from_trusted_context(
        Arc::clone(&workspace),
        actor(),
        SessionPlan::fresh(AgentSession::new("session-1").expect("session")),
        Some("debug-prompt-secret".to_owned()),
        AccessMode::WorkspaceOnly,
    )
    .with_capability_plan(plan.clone());
    let spec = launch_spec(AgentKind::Codex, "codex", &request).expect("Codex launch spec");

    for (debug_index, debug) in [
        format!("{machine:?}"),
        format!("{plan:?}"),
        format!("{request:?}"),
        format!("{spec:?}"),
    ]
    .into_iter()
    .enumerate()
    {
        for (secret_index, secret) in [
            "debug-capability-secret",
            "debug-prompt-secret",
            "https://private-endpoint.example.test/mcp",
        ]
        .into_iter()
        .enumerate()
        {
            assert!(
                !debug.contains(secret),
                "private Debug value present at surface {debug_index}, category {secret_index}"
            );
        }
    }
}
