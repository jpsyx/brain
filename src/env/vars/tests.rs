use super::*;
use crate::env::schema::{DEFAULT_CLAUDE_CMD, DEFAULT_CODEX_CMD, DEFAULT_OPENCODE_CMD, default_of};

/// A workspace root that never exists on disk, so root-based resolution reads no
/// real `config.json`.
const TEST_ROOT: &str = "/home/tester/brain";

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
fn setting_an_unknown_default_agent_frontend_is_rejected_before_any_write() {
    // `command()` points at a missing registry, so a value that reached the
    // store would fail there instead; this must fail on validation, naming the
    // whole valid set.
    let error = set(&command(), "default_agent_frontend", "gemini")
        .expect_err("unknown frontend")
        .to_string();

    assert!(error.contains("claude, codex, opencode"), "{error}");
    assert!(error.contains("gemini"), "{error}");
}

#[test]
fn a_hyphenated_open_code_default_is_stored_canonically() {
    let (_home, command) = registry_backed_command();

    set(&command, "default_agent_frontend", "Open-Code").expect("set frontend");

    // Stored canonically, so every reader sees an `AgentKind::as_str` value.
    assert_eq!(
        get(&command, "default_agent_frontend").as_deref(),
        Some("opencode")
    );
    assert_eq!(
        resolve_one(&command, "default_agent_frontend").as_deref(),
        Some("opencode")
    );
}

#[test]
fn an_unset_default_agent_frontend_resolves_to_claude() {
    let (_home, command) = registry_backed_command();

    assert_eq!(
        resolve_one(&command, "default_agent_frontend").as_deref(),
        Some("claude")
    );
}

/// A command context backed by a real single-workspace registry file, so `set`
/// round-trips through the store.
fn registry_backed_command() -> (tempfile::TempDir, CommandContext) {
    use std::collections::{BTreeMap, BTreeSet};

    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("brain");
    std::fs::create_dir_all(&root).unwrap();
    let name = crate::workspace::WorkspaceName::parse("brain").unwrap();
    let id = crate::workspace::WorkspaceId::parse("31e0f0f0-1c3b-4d7a-9f2e-8a5c6b7d8e90").unwrap();
    let registry = crate::workspace::MachineRegistry {
        schema_version: crate::workspace::REGISTRY_SCHEMA_VERSION,
        default_workspace: name.clone(),
        workspaces: BTreeMap::from([(
            name.clone(),
            crate::workspace::WorkspaceRecord {
                workspace_id: id,
                root: root.clone(),
                aliases: BTreeSet::new(),
                local_user_id: "pablo".to_owned(),
                receiver_enabled: false,
                env: Map::new(),
            },
        )]),
        env: serde_json::Map::new(),
    };
    let store =
        crate::workspace::RegistryStore::from_path(home.path().join("config/brain/env.json"));
    store.replace(&registry).unwrap();
    let command = CommandContext::for_test(
        std::sync::Arc::new(
            crate::workspace::WorkspaceContext::new(
                home.path(),
                id,
                name,
                &root,
                "pablo",
                home.path(),
            )
            .unwrap(),
        ),
        store,
        "pablo",
    );
    (home, command)
}

#[test]
fn the_markdown_to_pdf_path_is_stored_once_for_the_whole_machine() {
    let (_home, command) = registry_backed_command();

    set(&command, "markdown_to_pdf_path", "/opt/markdown-to-pdf").expect("set global");

    // It reads back through the ordinary env surface…
    assert_eq!(
        resolve_one(&command, "markdown_to_pdf_path").as_deref(),
        Some("/opt/markdown-to-pdf")
    );
    // …but lives in the registry's machine-global map, not in the workspace
    // record, so a second workspace on this machine cannot disagree with it.
    let registry = crate::workspace::RegistryStore::load_from(command.registry_store.path())
        .expect("registry");
    assert_eq!(
        registry
            .env
            .get("markdown_to_pdf_path")
            .and_then(Value::as_str),
        Some("/opt/markdown-to-pdf")
    );
    assert!(
        registry
            .workspaces
            .values()
            .all(|record| !record.env.contains_key("markdown_to_pdf_path"))
    );
}

#[test]
fn a_workspace_scoped_variable_still_lands_in_its_own_record() {
    let (_home, command) = registry_backed_command();

    set(&command, "claude_cmd", "claude --resume").expect("set workspace value");

    let registry = crate::workspace::RegistryStore::load_from(command.registry_store.path())
        .expect("registry");
    assert!(registry.env.get("claude_cmd").is_none());
    assert!(
        registry
            .workspaces
            .values()
            .any(|record| record.env.contains_key("claude_cmd"))
    );
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

mod secrets;
