use super::*;

#[test]
fn current_schema_json_parses_validated_string_types_and_defaults() {
    let registry: MachineRegistry = serde_json::from_value(json!({
        "schema_version": REGISTRY_SCHEMA_VERSION,
        "default_workspace": "brain",
        "workspaces": {
            "brain": {
                "workspace_id": PERSONAL_ID,
                "root": "/workspaces/brain",
                "local_user_id": "pablo"
            }
        }
    }))
    .expect("current-schema JSON");

    let record = registry.workspaces.get(&name("brain")).unwrap();
    assert_eq!(registry.schema_version, REGISTRY_SCHEMA_VERSION);
    assert_eq!(registry.default_workspace.as_str(), "brain");
    assert!(record.aliases.is_empty());
    assert!(!record.receiver_enabled);
    assert!(record.env.is_empty());
    assert_eq!(record.workspace_id.to_string(), PERSONAL_ID);
    // The machine-global map is optional; a file that omits it reads as empty.
    assert!(registry.env.is_empty());
}

#[test]
fn a_previous_schema_is_rejected_so_startup_migrates_it() {
    // An older schema is not readable as the current one: that rejection is
    // exactly what routes an older machine through the upgrade on its next
    // command.
    let error = serde_json::from_value::<MachineRegistry>(json!({
        "schema_version": 3,
        "default_workspace": "brain",
        "workspaces": {
            "brain": {
                "workspace_id": PERSONAL_ID,
                "root": "/workspaces/brain",
                "local_user_id": "pablo"
            }
        }
    }))
    .expect_err("an older schema must not load as current");

    assert!(error.to_string().contains("schema 3"), "{error}");
}

#[test]
fn the_machine_global_env_round_trips_and_is_omitted_when_empty() {
    let registry: MachineRegistry = serde_json::from_value(json!({
        "schema_version": REGISTRY_SCHEMA_VERSION,
        "default_workspace": "brain",
        "env": {"markdown_to_pdf_path": "/opt/markdown-to-pdf"},
        "workspaces": {
            "brain": {
                "workspace_id": PERSONAL_ID,
                "root": "/workspaces/brain",
                "local_user_id": "pablo"
            }
        }
    }))
    .expect("registry with machine-global env");

    assert_eq!(
        registry
            .env
            .get("markdown_to_pdf_path")
            .and_then(Value::as_str),
        Some("/opt/markdown-to-pdf")
    );
    let written = serde_json::to_value(&registry).expect("serialize");
    assert_eq!(
        written["env"]["markdown_to_pdf_path"],
        "/opt/markdown-to-pdf"
    );

    let mut empty = registry;
    empty.env.clear();
    let written = serde_json::to_value(&empty).expect("serialize");
    assert!(written.get("env").is_none(), "{written}");
}

#[test]
fn direct_deserialization_rejects_unsupported_schema_version() {
    let raw = format!(
        r#"{{
                "schema_version": 1,
                "default_workspace": "brain",
                "workspaces": {{
                    "brain": {{
                        "workspace_id": "{PERSONAL_ID}",
                        "root": "/workspaces/brain",
                        "local_user_id": "personal"
                    }}
                }}
            }}"#
    );

    assert!(serde_json::from_str::<MachineRegistry>(&raw).is_err());
}

#[test]
fn direct_deserialization_rejects_every_whole_registry_violation() {
    let mut empty = valid_registry_json();
    empty["workspaces"] = json!({});
    let mut missing_default = valid_registry_json();
    missing_default["default_workspace"] = json!("missing");
    let mut duplicate_selector = valid_registry_json();
    duplicate_selector["workspaces"]["brain"]["aliases"] = json!(["family"]);
    let mut duplicate_id = valid_registry_json();
    duplicate_id["workspaces"]["family"]["workspace_id"] = json!(PERSONAL_ID);
    let mut relative_root = valid_registry_json();
    relative_root["workspaces"]["family"]["root"] = json!("relative/family");
    let mut overlapping_root = valid_registry_json();
    overlapping_root["workspaces"]["family"]["root"] = json!("/workspaces/brain/family");

    for (case, value) in [
        ("empty", empty),
        ("missing default", missing_default),
        ("duplicate selector", duplicate_selector),
        ("duplicate UUID", duplicate_id),
        ("relative root", relative_root),
        ("overlapping root", overlapping_root),
    ] {
        assert!(
            serde_json::from_value::<MachineRegistry>(value).is_err(),
            "{case} must fail direct deserialization"
        );
    }
}

#[test]
fn workspace_name_deserialization_cannot_bypass_validation() {
    assert!(serde_json::from_str::<WorkspaceName>(r#""not a slug""#).is_err());
}

#[test]
fn workspace_id_deserialization_cannot_bypass_validation() {
    assert!(serde_json::from_str::<WorkspaceId>(r#""not-a-uuid""#).is_err());
}

#[test]
fn canonical_equivalent_duplicate_json_keys_are_rejected() {
    let raw = format!(
        r#"{{
                "schema_version": {REGISTRY_SCHEMA_VERSION},
                "default_workspace": "brain",
                "workspaces": {{
                    "brain": {{
                        "workspace_id": "{PERSONAL_ID}",
                        "root": "/workspaces/brain",
                        "local_user_id": "personal"
                    }},
                    "BRAIN": {{
                        "workspace_id": "{FAMILY_ID}",
                        "root": "/workspaces/family",
                        "local_user_id": "family"
                    }}
                }}
            }}"#
    );

    assert!(serde_json::from_str::<MachineRegistry>(&raw).is_err());
}

#[test]
fn canonical_equivalent_duplicate_aliases_in_json_are_rejected() {
    let raw = format!(
        r#"{{
                "schema_version": {REGISTRY_SCHEMA_VERSION},
                "default_workspace": "brain",
                "workspaces": {{
                    "brain": {{
                        "workspace_id": "{PERSONAL_ID}",
                        "root": "/workspaces/brain",
                        "aliases": ["fam", "FAM"],
                        "local_user_id": "personal"
                    }}
                }}
            }}"#
    );

    assert!(serde_json::from_str::<MachineRegistry>(&raw).is_err());
}

#[test]
fn unknown_fields_are_rejected_at_every_registry_level() {
    let mut registry_field = valid_registry_json();
    registry_field["unexpected"] = json!(true);
    let mut record_field = valid_registry_json();
    record_field["workspaces"]["brain"]["unexpected"] = json!(true);

    for value in [registry_field, record_field] {
        assert!(serde_json::from_value::<MachineRegistry>(value).is_err());
    }
}
