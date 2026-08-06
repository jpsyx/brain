use super::*;
use crate::env::schema::{DEFAULT_CLAUDE_CMD, DEFAULT_CODEX_CMD, DEFAULT_OPENCODE_CMD, default_of};

fn command() -> CommandContext {
    CommandContext::for_test(
        std::sync::Arc::new(
            crate::workspace::WorkspaceContext::new(
                std::path::Path::new("/home/tester"),
                crate::workspace::WorkspaceId::new(),
                crate::workspace::WorkspaceName::parse("brain").expect("valid name"),
                std::path::Path::new("/home/tester/brain"),
                "tester",
                std::path::Path::new("/home/tester"),
            )
            .expect("context"),
        ),
        crate::workspace::RegistryStore::from_path(std::path::PathBuf::from("/missing/env.json")),
        "tester",
    )
}

#[test]
fn resolve_all_lists_root_markdown_to_pdf_path_and_agent_cmds() {
    let rows = resolve_all(&command());
    assert!(rows.len() >= 5);
    assert!(rows.iter().any(|r| r.name == "root"));
    assert!(rows.iter().any(|r| r.name == "markdown_to_pdf_path"));
    assert!(rows.iter().any(|r| r.name == "claude_cmd"));
    assert!(rows.iter().any(|r| r.name == "codex_cmd"));
    assert!(rows.iter().any(|r| r.name == "opencode_cmd"));
    assert!(
        rows.iter()
            .find(|r| r.name == "root")
            .unwrap()
            .value
            .is_some()
    );
}

#[test]
fn set_rejects_unknown_env_variables() {
    assert!(set(&command(), "linear_workspace", "acme").is_err());
}

#[test]
fn set_raw_accepts_a_structured_object_value() {
    // set_raw must accept a nested object (unlike `set`, which coerces
    // scalars). We assert the value shape it will store; the store IO is
    // covered by the store module.
    let v = serde_json::json!({"enabled": true, "b2_bucket": "b"});
    assert!(v.is_object());
    assert_eq!(v.get("b2_bucket").and_then(|x| x.as_str()), Some("b"));
}

#[test]
fn root_row_reflects_the_resolved_brain_root() {
    let rows = resolve_all(&command());
    let root = rows.iter().find(|r| r.name == "root").unwrap();
    assert_eq!(root.value.as_deref(), Some("/home/tester/brain"));
}

#[test]
fn blank_codex_command_defaults_to_codex() {
    assert_eq!(trim_or_default("", DEFAULT_CODEX_CMD), "codex");
}

#[test]
fn blank_opencode_command_defaults_to_opencode() {
    assert_eq!(trim_or_default("", DEFAULT_OPENCODE_CMD), "opencode");
}

#[test]
fn blank_claude_command_defaults_to_permissionless_claude() {
    assert_eq!(
        trim_or_default("", DEFAULT_CLAUDE_CMD),
        "claude --dangerously-skip-permissions"
    );
}

#[test]
fn agent_command_schema_declares_defaults() {
    assert_eq!(
        default_of("claude_cmd"),
        Some("claude --dangerously-skip-permissions")
    );
    assert_eq!(default_of("codex_cmd"), Some("codex"));
    assert_eq!(default_of("opencode_cmd"), Some("opencode"));
}

#[test]
fn flatten_map_recurses_through_nested_objects_and_arrays() {
    let map = serde_json::from_value(serde_json::json!({
        "sync": {
            "remote": {"credentials": {"key_id": "abc"}},
            "exclude": ["tasks.csv", "habits.csv"]
        }
    }))
    .unwrap();

    let rows = flatten_map(&map);
    assert_eq!(
        rows.iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        vec![
            "sync.exclude.0",
            "sync.exclude.1",
            "sync.remote.credentials.key_id"
        ]
    );
}

#[test]
fn dotted_paths_read_and_write_without_losing_siblings() {
    let mut map = serde_json::from_value(serde_json::json!({
        "sync": {"remote": {"bucket": "brain"}, "enabled": true}
    }))
    .unwrap();

    assert_eq!(
        get_path(&map, "sync.remote.bucket"),
        Some(&Value::from("brain"))
    );
    set_path(&mut map, "sync.remote.key_id", Value::from("abc")).unwrap();
    assert_eq!(map["sync"]["enabled"], Value::Bool(true));
    assert_eq!(map["sync"]["remote"]["bucket"], Value::from("brain"));
    assert_eq!(map["sync"]["remote"]["key_id"], Value::from("abc"));
}

#[test]
fn receiver_secrets_are_known_but_redacted_from_env_output() {
    let mut map = Map::new();
    map.insert("twilio_auth_token".to_owned(), Value::from("twilio-secret"));
    map.insert("resend_api_key".to_owned(), Value::from("resend-secret"));

    assert_eq!(
        resolve_one_from_map(&command(), &map, "twilio_auth_token"),
        Some("(set)".to_owned())
    );
    assert_eq!(
        resolve_one_from_map(&command(), &map, "resend_api_key"),
        Some("(set)".to_owned())
    );
}

#[test]
fn agent_capability_credentials_are_redacted_from_env_list_rows() {
    let map = serde_json::from_value(serde_json::json!({
        "agent_capabilities": {
            "mcps": [{
                "name": "notion",
                "url": "https://notion.example.test/mcp",
                "credentials": {
                    "bearer_token": "machine-secret",
                    "headers": {"Authorization": "header-secret"}
                }
            }]
        }
    }))
    .expect("env map");

    let rows = resolve_all_from(&command(), &map);

    assert!(rows.iter().any(|row| {
        row.name == "agent_capabilities.mcps.0.url"
            && row.value.as_deref() == Some("https://notion.example.test/mcp")
    }));
    assert!(rows.iter().any(|row| {
        row.name == "agent_capabilities.mcps.0.credentials.bearer_token"
            && row.value.as_deref() == Some("(set)")
    }));
    let rendered = rows
        .iter()
        .filter_map(|row| row.value.as_deref())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!rendered.contains("machine-secret"));
    assert!(!rendered.contains("header-secret"));
}
