use brain::access::{AccessMode, MachineCapabilityEnvironment, capability_plan};
use brain::config::Config;

use crate::support::family_id;

#[test]
fn reserved_frontend_environment_names_make_stdio_connections_unavailable() {
    for reserved in [
        "HOME",
        "PATH",
        "CODEX_HOME",
        "BRAIN_WORKSPACE_ID",
        "BRAIN_TRIAGE_TOKEN",
    ] {
        let config = Config {
            access_mode: AccessMode::WorkspaceOnly,
            allowed_mcps: vec!["notion".to_owned()],
            allowed_skills: Vec::new(),
            ..Config::default()
        };
        let machine = MachineCapabilityEnvironment::from_value(
            family_id(),
            serde_json::json!({
                "mcps": [{
                    "name": "notion",
                    "command": "/usr/bin/true",
                    "credentials": {"environment": {reserved: "machine-secret"}}
                }]
            }),
        )
        .expect("machine capability environment");

        let plan = capability_plan(&config, &machine).expect("capability plan");

        assert!(
            plan.mcps.available_names().is_empty(),
            "reserved {reserved}"
        );
        assert!(
            plan.mcps
                .unavailable_reason("notion")
                .is_some_and(|reason| reason.contains("reserved")),
            "reserved {reserved}"
        );
    }
}

#[test]
fn duplicate_portable_logical_names_are_configuration_errors() {
    let config = Config {
        access_mode: AccessMode::WorkspaceOnly,
        allowed_mcps: vec!["notion".to_owned(), "notion".to_owned()],
        ..Config::default()
    };
    let machine = MachineCapabilityEnvironment::from_value(
        family_id(),
        serde_json::json!({"mcps": [{"name": "notion", "url": "https://example.test/mcp"}]}),
    )
    .expect("machine capability environment");

    let error = capability_plan(&config, &machine).expect_err("duplicate must fail");

    assert!(
        error
            .to_string()
            .contains("duplicate allowed_mcps name `notion`")
    );
}

#[test]
fn logical_names_are_ascii_normalized_and_duplicates_are_checked_after_normalization() {
    let machine = MachineCapabilityEnvironment::from_value(
        family_id(),
        serde_json::json!({
            "mcps": [{"name": "NOTION", "url": "https://example.test/mcp"}]
        }),
    )
    .expect("machine capability environment");
    let normalized = capability_plan(
        &Config {
            access_mode: AccessMode::WorkspaceOnly,
            allowed_mcps: vec!["Notion".to_owned()],
            allowed_skills: Vec::new(),
            ..Config::default()
        },
        &machine,
    )
    .expect("normalized capability plan");
    assert_eq!(normalized.mcps.names(), ["notion"]);
    assert_eq!(normalized.mcps.available_names(), ["notion"]);

    let duplicate = capability_plan(
        &Config {
            access_mode: AccessMode::WorkspaceOnly,
            allowed_mcps: vec!["Notion".to_owned(), "notion".to_owned()],
            allowed_skills: Vec::new(),
            ..Config::default()
        },
        &machine,
    )
    .expect_err("case-folded duplicate must fail");
    assert!(
        duplicate
            .to_string()
            .contains("duplicate allowed_mcps name `notion`")
    );
}

#[test]
fn logical_names_reject_whitespace_controls_unicode_and_shell_punctuation() {
    let machine =
        MachineCapabilityEnvironment::from_value(family_id(), serde_json::json!({"mcps": []}))
            .expect("machine capability environment");
    for invalid in [" notion", "not ion", "notion\n", "nøtion", "notion;rm"] {
        let error = capability_plan(
            &Config {
                access_mode: AccessMode::WorkspaceOnly,
                allowed_mcps: vec![invalid.to_owned()],
                allowed_skills: Vec::new(),
                ..Config::default()
            },
            &machine,
        )
        .expect_err("invalid logical name must fail");
        assert!(error.to_string().contains("allowed_mcps"), "{invalid:?}");
    }
}

#[test]
fn malformed_mcp_transport_data_is_unavailable_instead_of_entering_launch_artifacts() {
    let config = Config {
        access_mode: AccessMode::WorkspaceOnly,
        allowed_mcps: vec![
            "spaced-command".to_owned(),
            "control-arg".to_owned(),
            "bad-scheme".to_owned(),
            "missing-host".to_owned(),
            "fragment".to_owned(),
            "header-injection".to_owned(),
        ],
        allowed_skills: Vec::new(),
        ..Config::default()
    };
    let machine = MachineCapabilityEnvironment::from_value(
        family_id(),
        serde_json::json!({
            "mcps": [
                {"name": "spaced-command", "command": "/opt/MCP server"},
                {"name": "control-arg", "command": "/usr/bin/mcp", "args": ["ok\nno"]},
                {"name": "bad-scheme", "url": "file:///tmp/socket"},
                {"name": "missing-host", "url": "https:///mcp"},
                {"name": "fragment", "url": "https://example.test/mcp#ignored"},
                {
                    "name": "header-injection",
                    "url": "https://example.test/mcp",
                    "credentials": {"headers": {"Authorization\nInjected": "secret"}}
                }
            ]
        }),
    )
    .expect("machine capability environment");

    let plan = capability_plan(&config, &machine).expect("capability plan");

    assert!(plan.mcps.available_names().is_empty());
    for name in &config.allowed_mcps {
        assert!(
            plan.mcps.unavailable_reason(name).is_some(),
            "{name} should be unavailable"
        );
    }
}
