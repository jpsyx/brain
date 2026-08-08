use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde_json::{Map, Value, json};

use super::{Breakdown, assemble};
use crate::workspace::{MachineRegistry, WorkspaceId, WorkspaceName, WorkspaceRecord};

fn name(raw: &str) -> WorkspaceName {
    WorkspaceName::parse(raw).expect("valid workspace name")
}

fn record(id: WorkspaceId, root: &str, env: Value) -> WorkspaceRecord {
    WorkspaceRecord {
        workspace_id: id,
        root: PathBuf::from(root),
        aliases: BTreeSet::new(),
        local_user_id: "tester".to_owned(),
        receiver_enabled: false,
        env: serde_json::from_value(env).expect("env object"),
    }
}

/// A two-workspace machine: `brain` (default) and `family`, each with its own
/// env, mirroring the shape `env.json` really has.
fn fixture() -> (MachineRegistry, WorkspaceId, WorkspaceId) {
    let brain_id = WorkspaceId::new();
    let family_id = WorkspaceId::new();
    let registry = MachineRegistry {
        schema_version: 2,
        default_workspace: name("brain"),
        workspaces: BTreeMap::from([
            (
                name("brain"),
                record(
                    brain_id,
                    "/home/tester/brain",
                    json!({"claude_cmd": "claude --brain", "sync": {"b2_bucket": "brain-bucket"}}),
                ),
            ),
            (
                name("family"),
                record(
                    family_id,
                    "/home/tester/family",
                    json!({"twilio_auth_token": "family-secret"}),
                ),
            ),
        ]),
    };
    (registry, brain_id, family_id)
}

fn raw_object() -> Map<String, Value> {
    serde_json::from_value(json!({
        "schema_version": 2,
        "default_workspace": "brain",
        "workspaces": {"brain": {"root": "/home/tester/brain"}},
    }))
    .expect("raw registry object")
}

fn breakdown() -> Breakdown {
    let (registry, brain_id, _) = fixture();
    assemble(
        PathBuf::from("/home/tester/.config/brain/env.json"),
        &raw_object(),
        Some(&registry),
        brain_id,
    )
}

fn row<'a>(rows: &'a [crate::settings::Resolved], name: &str) -> &'a crate::settings::Resolved {
    rows.iter()
        .find(|row| row.name == name)
        .unwrap_or_else(|| panic!("row {name} missing"))
}

#[test]
fn global_rows_are_every_top_level_key_except_workspaces() {
    let breakdown = breakdown();

    let names = breakdown
        .global
        .iter()
        .map(|row| row.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, ["default_workspace", "schema_version"]);
    assert!(
        !names.iter().any(|name| name.starts_with("workspaces")),
        "{names:?}"
    );
    assert_eq!(
        row(&breakdown.global, "schema_version").value.as_deref(),
        Some("2")
    );
    assert_eq!(
        row(&breakdown.global, "default_workspace").value.as_deref(),
        Some("brain")
    );
}

#[test]
fn an_undeclared_machine_global_key_still_lists_with_a_generic_description() {
    let (registry, brain_id, _) = fixture();
    let mut raw = raw_object();
    raw.insert("future_global".to_owned(), json!("kept"));

    let breakdown = assemble(PathBuf::new(), &raw, Some(&registry), brain_id);

    let future = row(&breakdown.global, "future_global");
    assert_eq!(future.value.as_deref(), Some("kept"));
    assert_eq!(future.description, super::GLOBAL_FALLBACK_DESCRIPTION);
}

#[test]
fn every_registered_workspace_gets_a_block_marked_default_and_selected() {
    let (registry, _, family_id) = fixture();

    let breakdown = assemble(PathBuf::new(), &raw_object(), Some(&registry), family_id);

    let blocks = breakdown
        .workspaces
        .iter()
        .map(|block| (block.name.as_str(), block.is_default, block.is_selected))
        .collect::<Vec<_>>();
    assert_eq!(blocks, [("brain", true, false), ("family", false, true)]);
}

#[test]
fn each_block_resolves_its_own_root_and_its_own_env_never_a_peers() {
    let breakdown = breakdown();
    let brain = &breakdown.workspaces[0];
    let family = &breakdown.workspaces[1];

    assert_eq!(
        row(&brain.rows, "root").value.as_deref(),
        Some("/home/tester/brain")
    );
    assert_eq!(
        row(&family.rows, "root").value.as_deref(),
        Some("/home/tester/family")
    );
    assert_eq!(
        row(&brain.rows, "claude_cmd").value.as_deref(),
        Some("claude --brain")
    );
    // family never set claude_cmd, so it falls back to the shipped default
    // instead of inheriting the selected workspace's override.
    assert_eq!(
        row(&family.rows, "claude_cmd").value.as_deref(),
        Some(crate::env::schema::DEFAULT_CLAUDE_CMD)
    );
    assert!(brain.rows.iter().any(|row| row.name == "sync.b2_bucket"));
    assert!(!family.rows.iter().any(|row| row.name == "sync.b2_bucket"));
}

#[test]
fn every_declared_variable_appears_in_every_block_even_when_unset() {
    let breakdown = breakdown();

    for block in &breakdown.workspaces {
        assert!(
            row(&block.rows, "resend_api_key").value.is_none(),
            "{} should list resend_api_key as unset",
            block.name
        );
        assert!(row(&block.rows, "markdown_to_pdf_path").value.is_none());
    }
}

#[test]
fn a_non_selected_workspaces_secret_is_redacted() {
    let breakdown = breakdown();
    let family = &breakdown.workspaces[1];

    assert_eq!(
        row(&family.rows, "twilio_auth_token").value.as_deref(),
        Some("(set)")
    );
    let rendered = family
        .rows
        .iter()
        .filter_map(|row| row.value.as_deref())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!rendered.contains("family-secret"), "{rendered}");
}

#[test]
fn variables_document_every_global_and_declared_row_but_no_nested_path() {
    let breakdown = breakdown();

    let documented = breakdown
        .variables
        .iter()
        .map(|doc| doc.name.as_str())
        .collect::<Vec<_>>();
    assert!(documented.contains(&"schema_version"), "{documented:?}");
    assert!(documented.contains(&"default_workspace"), "{documented:?}");
    for declared in ["root", "claude_cmd", "resend_api_key"] {
        assert!(documented.contains(&declared), "{documented:?}");
    }
    assert!(
        !documented.iter().any(|name| name.contains('.')),
        "{documented:?}"
    );
    assert!(
        breakdown
            .variables
            .iter()
            .all(|doc| !doc.description.is_empty())
    );
}

#[test]
fn an_unreadable_registry_yields_an_empty_view_instead_of_failing() {
    let breakdown = assemble(PathBuf::new(), &Map::new(), None, WorkspaceId::new());

    assert!(breakdown.global.is_empty());
    assert!(breakdown.workspaces.is_empty());
}
