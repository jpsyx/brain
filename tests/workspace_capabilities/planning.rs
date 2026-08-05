use std::collections::{BTreeMap, BTreeSet};

use brain::access::{
    AccessMode, CapabilityEnforcement, EnforcementEvidence, MachineCapabilityEnvironment,
    capability_plan, capability_plan_for,
};
use brain::config::Config;
use brain::workspace::{
    CommandContext, MachineRegistry, RegistryStore, WorkspaceId, WorkspaceName, WorkspaceRecord,
};

use crate::support::{family_id, temporary_workspace};

#[test]
fn missing_skill_allowlist_gets_core_defaults_but_explicit_empty_stays_empty() {
    let missing: Config = serde_json::from_str(r#"{"access_mode":"workspace_only"}"#)
        .expect("config with missing skill allowlist");
    let empty: Config =
        serde_json::from_str(r#"{"access_mode":"workspace_only","allowed_skills":[]}"#)
            .expect("config with explicit empty skill allowlist");

    assert_eq!(
        missing.allowed_skills,
        ["contacts", "second-brain", "todo", "triage"]
    );
    assert!(empty.allowed_skills.is_empty());
}

#[test]
fn logical_allowlists_resolve_separately_from_machine_connection_material() {
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
                        "headers": {"Authorization": "Bearer machine-only"}
                    }
                },
                {"name": "linear", "command": "/opt/local/bin/linear-mcp"},
                {"name": "superhuman", "url": "https://mail.example.test/mcp"}
            ],
            "skills": []
        }),
    )
    .expect("machine capability environment");

    let plan = capability_plan(&config, &machine).expect("capability plan");

    assert_eq!(plan.mcps.names(), ["notion"]);
    assert_eq!(
        plan.skills.names(),
        ["contacts", "second-brain", "todo", "triage"]
    );
    assert!(!plan.mcps.names().contains(&"linear"));
    assert!(!plan.mcps.names().contains(&"superhuman"));
    assert_eq!(plan.credentials.source_workspace(), family_id());
}

#[test]
fn requested_mcp_with_missing_machine_credential_is_unavailable() {
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
                "command": "/opt/local/bin/notion-mcp",
                "credentials": {"environment": {"NOTION_TOKEN": null}}
            }]
        }),
    )
    .expect("machine capability environment");

    let plan = capability_plan(&config, &machine).expect("capability plan");

    assert!(plan.mcps.available_names().is_empty());
    assert!(
        plan.mcps
            .unavailable_reason("notion")
            .is_some_and(|reason| reason.contains("credential"))
    );
}

#[test]
fn unrestricted_mode_uses_normal_frontend_global_configuration() {
    let config = Config {
        access_mode: AccessMode::Unrestricted,
        allowed_mcps: vec!["notion".to_owned()],
        allowed_skills: vec!["todo".to_owned()],
        ..Config::default()
    };
    let machine = MachineCapabilityEnvironment::from_value(family_id(), serde_json::json!({}))
        .expect("machine capability environment");

    let plan = capability_plan(&config, &machine).expect("capability plan");

    assert!(plan.mcps.uses_global_configuration());
    assert!(plan.skills.uses_global_configuration());
    assert!(plan.mcps.names().is_empty());
    assert!(plan.skills.names().is_empty());
}

#[test]
fn frontend_report_never_claims_strict_selection_without_exclusion_evidence() {
    let config = Config {
        access_mode: AccessMode::WorkspaceOnly,
        allowed_mcps: vec!["notion".to_owned(), "missing".to_owned()],
        ..Config::default()
    };
    let machine = MachineCapabilityEnvironment::from_value(
        family_id(),
        serde_json::json!({
            "mcps": [{"name": "notion", "url": "https://example.test/mcp"}]
        }),
    )
    .expect("machine capability environment");
    let plan = capability_plan(&config, &machine).expect("capability plan");

    let advisory = plan.enforcement_report(EnforcementEvidence::advisory_only());
    let strict_mcp = plan.enforcement_report(EnforcementEvidence::strict_mcps_only());

    assert_eq!(
        advisory.mcps.enforcement("notion"),
        Some(CapabilityEnforcement::AdvisoryOnly)
    );
    assert_eq!(
        advisory.skills.enforcement("todo"),
        Some(CapabilityEnforcement::AdvisoryOnly)
    );
    assert_eq!(
        strict_mcp.mcps.enforcement("notion"),
        Some(CapabilityEnforcement::StrictlySelected)
    );
    assert_eq!(
        strict_mcp.skills.enforcement("todo"),
        Some(CapabilityEnforcement::AdvisoryOnly)
    );
    assert_eq!(
        strict_mcp.mcps.enforcement("missing"),
        Some(CapabilityEnforcement::Unavailable)
    );
}

#[test]
fn config_command_storage_preserves_logical_lists_as_json_arrays() {
    let (_home, workspace) = temporary_workspace();

    brain::settings::set(&workspace, "allowed_mcps", "notion, linear").expect("set MCP allowlist");
    brain::settings::set(&workspace, "allowed_skills", "").expect("disable skills");

    let stored: serde_json::Value = serde_json::from_slice(
        &std::fs::read(workspace.root().join(".config/config.json")).expect("portable config"),
    )
    .expect("portable config JSON");
    assert_eq!(
        stored["allowed_mcps"],
        serde_json::json!(["notion", "linear"])
    );
    assert_eq!(stored["allowed_skills"], serde_json::json!([]));
}

#[test]
fn selected_registry_record_is_the_only_machine_capability_source() {
    let (home, workspace) = temporary_workspace();
    let portable_path = workspace.root().join(".config/config.json");
    std::fs::write(
        &portable_path,
        r#"{"access_mode":"workspace_only","allowed_mcps":["notion"]}"#,
    )
    .expect("portable config");
    let personal_id =
        WorkspaceId::parse("6fd873b7-f05a-4eb1-b92e-4b8ae3df8e11").expect("personal workspace id");
    let personal_name = WorkspaceName::parse("personal").expect("personal name");
    let personal_root = home.path().join("personal");
    std::fs::create_dir_all(&personal_root).expect("personal root");
    let family_name = WorkspaceName::parse("family").expect("family name");
    let store = RegistryStore::from_path(home.path().join(".config/brain/env.json"));
    let registry = MachineRegistry {
        schema_version: 2,
        default_workspace: family_name.clone(),
        workspaces: BTreeMap::from([
            (
                family_name,
                WorkspaceRecord {
                    workspace_id: family_id(),
                    root: workspace.root().to_path_buf(),
                    aliases: BTreeSet::new(),
                    local_user_id: "pablo".to_owned(),
                    receiver_enabled: false,
                    env: serde_json::Map::from_iter([(
                        "agent_capabilities".to_owned(),
                        serde_json::json!({
                            "mcps": [{
                                "name": "notion",
                                "url": "https://family.example.test/mcp",
                                "credentials": {"bearer_token": "family-secret"}
                            }]
                        }),
                    )]),
                },
            ),
            (
                personal_name,
                WorkspaceRecord {
                    workspace_id: personal_id,
                    root: personal_root,
                    aliases: BTreeSet::new(),
                    local_user_id: "pablo".to_owned(),
                    receiver_enabled: false,
                    env: serde_json::Map::from_iter([(
                        "agent_capabilities".to_owned(),
                        serde_json::json!({
                            "mcps": [{
                                "name": "notion",
                                "url": "https://personal.example.test/mcp",
                                "credentials": {"bearer_token": "personal-secret"}
                            }]
                        }),
                    )]),
                },
            ),
        ]),
    };
    store.replace(&registry).expect("write registry");
    let before = std::fs::read(store.path()).expect("registry bytes");
    let portable_before = std::fs::read(&portable_path).expect("portable config bytes");
    let context = CommandContext::new(workspace, store.clone()).expect("command context");
    let config = Config {
        access_mode: AccessMode::WorkspaceOnly,
        allowed_mcps: vec!["notion".to_owned()],
        ..Config::default()
    };

    let plan = capability_plan_for(&config, &context).expect("selected capability plan");

    assert_eq!(plan.credentials.source_workspace(), family_id());
    assert_eq!(plan.mcps.available_names(), ["notion"]);
    assert_eq!(std::fs::read(store.path()).expect("registry after"), before);
    assert_eq!(
        std::fs::read(&portable_path).expect("portable config after"),
        portable_before
    );
    assert!(!workspace_config_contains_secret(context.workspace.root()));
}

fn workspace_config_contains_secret(root: &std::path::Path) -> bool {
    std::fs::read(root.join(".config/config.json"))
        .ok()
        .is_some_and(|bytes| {
            let text = String::from_utf8_lossy(&bytes);
            text.contains("family-secret") || text.contains("personal-secret")
        })
}
