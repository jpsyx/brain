//! Reading and writing individual variables: name canonicalization, the
//! JSON↔string coercion typed readers rely on, and the get / set / resolve
//! operations behind `brain config`.

use anyhow::{Result, bail};
use serde_json::{Map, Value};

use super::schema::{Resolved, VARS, default_of, is_known, known_names};
use super::store::{load_map, save_map};
use crate::workspace::WorkspaceContext;

/// Canonicalize a variable name: lowercase, trimmed, dashes to underscores.
#[must_use]
pub fn normalize_name(name: &str) -> String {
    name.trim().to_ascii_lowercase().replace('-', "_")
}

/// Render a JSON value as the flat string the CLI and typed readers see.
fn value_to_string(name: &str, value: &Value) -> Option<String> {
    if name == "allowed_sms_senders"
        && let Value::Number(number) = value
        && number.as_u64().is_some_and(|number| number > 0)
    {
        return Some(format!("+{number}"));
    }

    match value {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        other => Some(other.to_string()),
    }
}

/// The raw explicit value for `name` (no default fallback).
#[must_use]
pub fn get(workspace: &WorkspaceContext, name: &str) -> Option<String> {
    load_map(workspace)
        .get(name)
        .and_then(|value| value_to_string(name, value))
}

/// The effective value for a known variable: explicit override else default.
#[must_use]
pub fn resolve_one(workspace: &WorkspaceContext, name: &str) -> Option<String> {
    if !is_known(name) {
        return None;
    }
    get(workspace, name).or_else(|| default_of(name).map(str::to_owned))
}

/// Coerce a raw CLI string into the tightest JSON type so typed readers keep
/// working (`day_rollover_hour=4` must round-trip as a number, not `"4"`).
fn parse_value(raw: &str) -> Value {
    if let Ok(i) = raw.parse::<i64>()
        && raw == i.to_string()
    {
        return Value::from(i);
    }
    match raw {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        other => Value::from(other),
    }
}

/// Persist `name=value` for a declared variable. Unknown names are rejected so
/// a typo can't silently rot in the store.
pub fn set(workspace: &WorkspaceContext, name: &str, value: &str) -> Result<()> {
    if !is_known(name) {
        bail!(
            "unknown config variable `{name}` (known: {})",
            known_names()
        );
    }
    if name == "enable_triage_habits" {
        let enabled = match value.trim() {
            "true" => true,
            "false" => false,
            _ => bail!("enable_triage_habits must be true or false"),
        };
        let owner = crate::tasks::store_lock::TaskStoreOwner::acquire(workspace)?;
        return crate::tasks::triage_habits::apply_triage_habits_config_owned(
            workspace, enabled, &owner,
        );
    }
    let owner = crate::tasks::store_lock::TaskStoreOwner::acquire(workspace)?;
    if name == "access_mode" {
        let mode = crate::access::AccessMode::parse(value)
            .ok_or_else(|| anyhow::anyhow!("access_mode must be unrestricted or workspace_only"))?;
        owner.verify(workspace)?;
        return crate::access::set_portable_access_mode(workspace.root(), mode);
    }
    let mut map = load_map(workspace);
    map.insert(name.to_owned(), parse_value(value));
    save_map(workspace, &map, &owner)
}

/// Every declared variable with its resolved value, in schema order.
#[must_use]
pub fn resolve_all(workspace: &WorkspaceContext) -> Vec<Resolved> {
    resolve_all_from(&load_map(workspace))
}

/// Pure core of [`resolve_all`]: resolve against an explicit map so the schema
/// and default logic are testable without touching the real store.
pub(super) fn resolve_all_from(map: &Map<String, Value>) -> Vec<Resolved> {
    VARS.iter()
        .map(|v| Resolved {
            name: v.name.to_owned(),
            value: map
                .get(v.name)
                .and_then(|value| value_to_string(v.name, value))
                .or_else(|| v.default.map(str::to_owned)),
            description: v.description.to_owned(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_workspace() -> (tempfile::TempDir, WorkspaceContext) {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("brain");
        std::fs::create_dir_all(root.join(".config")).unwrap();
        let workspace = WorkspaceContext::new(
            temporary.path(),
            crate::workspace::WorkspaceId::new(),
            crate::workspace::WorkspaceName::parse("brain").unwrap(),
            &root,
            "tester",
            temporary.path(),
        )
        .unwrap();
        (temporary, workspace)
    }

    fn workspace() -> WorkspaceContext {
        WorkspaceContext::new(
            std::path::Path::new("/home/tester"),
            crate::workspace::WorkspaceId::new(),
            crate::workspace::WorkspaceName::parse("brain").expect("valid name"),
            std::path::Path::new("/home/tester/brain"),
            "tester",
            std::path::Path::new("/home/tester"),
        )
        .expect("context")
    }

    #[test]
    fn normalize_lowercases_and_underscores() {
        assert_eq!(normalize_name("  Linear-Workspace "), "linear_workspace");
        assert_eq!(normalize_name("ROOT"), "root");
    }

    #[test]
    fn parse_value_tightens_numbers_and_bools() {
        assert_eq!(parse_value("4"), Value::from(4));
        assert_eq!(parse_value("true"), Value::Bool(true));
        assert_eq!(parse_value("~/brain"), Value::from("~/brain"));
        // A slug that merely starts with a digit stays a string.
        assert_eq!(parse_value("2acme"), Value::from("2acme"));
    }

    #[test]
    fn parse_value_preserves_an_e164_leading_plus() {
        assert_eq!(parse_value("+16072809118"), Value::from("+16072809118"));
    }

    #[test]
    fn legacy_numeric_sms_allowlist_renders_with_its_leading_plus() {
        let mut map = Map::new();
        map.insert(
            "allowed_sms_senders".to_owned(),
            Value::from(16_072_809_118_i64),
        );

        let rows = resolve_all_from(&map);
        let allowed = rows
            .iter()
            .find(|row| row.name == "allowed_sms_senders")
            .unwrap();

        assert_eq!(allowed.value.as_deref(), Some("+16072809118"));
    }

    #[test]
    fn malformed_nonpositive_numeric_sms_allowlist_is_not_given_a_plus() {
        let mut map = Map::new();
        map.insert("allowed_sms_senders".to_owned(), Value::from(-1));

        let rows = resolve_all_from(&map);
        let allowed = rows
            .iter()
            .find(|row| row.name == "allowed_sms_senders")
            .unwrap();

        assert_eq!(allowed.value.as_deref(), Some("-1"));
    }

    #[test]
    fn resolve_all_covers_every_var_and_applies_defaults() {
        // Hermetic: resolve against an empty map, not the real store.
        let rows = resolve_all_from(&Map::new());
        assert_eq!(rows.len(), VARS.len());
        // A var with a built-in default resolves to it.
        let agenda = rows.iter().find(|r| r.name == "agenda_dir").unwrap();
        assert_eq!(agenda.value.as_deref(), Some("~/Downloads"));
        // No built-in default → unset (until the user or discovery sets it).
        let ws = rows.iter().find(|r| r.name == "linear_workspace").unwrap();
        assert_eq!(ws.value, None);
    }

    #[test]
    fn root_is_not_a_config_variable() {
        // The brain-root pointer is resolved outside the config system, so
        // `config` neither lists nor accepts it.
        assert!(
            resolve_all_from(&Map::new())
                .iter()
                .all(|r| r.name != "root")
        );
        assert!(set(&workspace(), "root", "/srv/brain").is_err());
    }

    #[test]
    fn markdown_to_pdf_path_is_no_longer_a_brain_config_variable() {
        // It moved to brain env; `brain config` must reject it.
        assert!(
            resolve_all_from(&Map::new())
                .iter()
                .all(|r| r.name != "markdown_to_pdf_path")
        );
        assert!(set(&workspace(), "markdown_to_pdf_path", "/x").is_err());
    }

    #[test]
    fn claude_cmd_is_no_longer_a_brain_config_variable() {
        // Agent launch commands are machine-local, so `brain config` must
        // reject them and `brain env` owns the user-facing setting.
        assert!(
            resolve_all_from(&Map::new())
                .iter()
                .all(|r| r.name != "claude_cmd")
        );
        assert!(set(&workspace(), "claude_cmd", "claude").is_err());
    }

    #[test]
    fn skills_auto_sync_defaults_on_after_the_b4_cutover() {
        // The rollout gate is flipped: with nothing set, auto-sync is on so a
        // config/personalize mutation re-renders the live registry (invariant #5).
        let rows = resolve_all_from(&Map::new());
        let flag = rows.iter().find(|r| r.name == "skills_auto_sync").unwrap();
        assert_eq!(flag.value.as_deref(), Some("true"));
    }

    #[test]
    fn resolve_all_prefers_an_explicit_value_over_the_default() {
        let mut map = Map::new();
        map.insert("agenda_dir".to_owned(), Value::from("/srv/agenda"));
        map.insert("linear_workspace".to_owned(), Value::from("acme"));
        let rows = resolve_all_from(&map);
        let val = |n: &str| rows.iter().find(|r| r.name == n).unwrap().value.clone();
        assert_eq!(val("agenda_dir").as_deref(), Some("/srv/agenda"));
        assert_eq!(val("linear_workspace").as_deref(), Some("acme"));
    }

    #[test]
    fn config_read_modify_write_waits_for_the_workspace_task_store_owner() {
        let (_temporary, workspace) = temporary_workspace();
        let owner = crate::tasks::store_lock::TaskStoreOwner::acquire(&workspace).unwrap();
        let writer_workspace = workspace.clone();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let writer = std::thread::spawn(move || {
            done_tx
                .send(set(&writer_workspace, "day_rollover_hour", "4"))
                .unwrap();
        });

        assert!(
            done_rx
                .recv_timeout(std::time::Duration::from_millis(200))
                .is_err(),
            "config writer entered while another task-store owner was active"
        );
        drop(owner);

        done_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap()
            .unwrap();
        writer.join().unwrap();
        assert_eq!(
            resolve_one(&workspace, "day_rollover_hour").as_deref(),
            Some("4")
        );
    }
}
