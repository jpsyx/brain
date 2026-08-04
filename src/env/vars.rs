//! Reading and writing brain-env variables: get / set / resolve behind
//! `brain env`. Mirrors `settings::vars` but over the env store, and renders
//! into the shared `settings::Resolved` type. Nested JSON objects are exposed
//! as dot-separated paths so the full env store remains inspectable.

use anyhow::{Result, bail};
use serde_json::{Map, Value};

use super::schema::{VARS, is_known, known_names};
use super::store::{load_map, save_map};
use crate::settings::Resolved;
use crate::workspace::CommandContext;

fn value_to_string(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

/// The raw explicit value for `name` (no default fallback).
#[must_use]
pub fn get(command: &CommandContext, name: &str) -> Option<String> {
    let map = load_map(command);
    if !name.contains('.') {
        return map.get(name).and_then(value_to_string);
    }
    get_path(&map, name).and_then(value_to_string)
}

/// The effective value for a known env variable: explicit override else default.
///
/// `root` resolves through [`crate::paths::brain_root_path`] so the shown value
/// matches what brain actually uses (including the legacy-pointer fallback).
#[must_use]
pub fn resolve_one(command: &CommandContext, name: &str) -> Option<String> {
    if name == "root" {
        return Some(command.workspace.root().display().to_string());
    }
    if name.contains('.') {
        return get(command, name);
    }
    if !is_known(name) {
        return None;
    }
    let spec = VARS.iter().find(|spec| spec.name == name)?;
    let value = get(command, name).or_else(|| {
        spec.legacy_config_fallback
            .then(|| legacy_config_value(command.workspace.as_ref(), name))
            .flatten()
    });
    match (value, spec.default) {
        (Some(value), Some(default)) => Some(trim_or_default(&value, default)),
        (Some(value), None) => Some(value),
        (None, Some(default)) => Some(default.to_owned()),
        (None, None) => None,
    }
}

fn trim_or_default(cmd: &str, default: &str) -> String {
    let trimmed = cmd.trim();
    if trimmed.is_empty() {
        default.to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn legacy_config_value(
    workspace: &crate::workspace::WorkspaceContext,
    name: &str,
) -> Option<String> {
    crate::settings::load_map(workspace)
        .get(name)
        .and_then(value_to_string)
        .and_then(|cmd| {
            let trimmed = cmd.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        })
}

/// Persist `name=value` into the env store. Dotted names address nested JSON
/// objects, preserving all sibling values along the path.
pub fn set(command: &CommandContext, name: &str, value: &str) -> Result<()> {
    let segments = path_segments(name)?;
    if super::schema::is_structural(name) {
        bail!("env variable `{name}` is structural and read-only");
    }
    let mut map = load_map(command);
    if segments.len() == 1 && !is_known(name) {
        bail!("unknown env variable `{name}` (known: {})", known_names());
    }
    if segments.len() > 1 {
        let top = segments[0];
        let top_value = map.get(top);
        let can_descend = top == "sync" || is_known(top) || top_value.is_some_and(Value::is_object);
        if !can_descend {
            bail!("unknown env object `{top}` (known: {})", known_names());
        }
    }
    set_path(&mut map, name, parse_value(value))?;
    save_map(command, &map)
}

/// Write a raw JSON value under `name`, bypassing the declared-variable check.
///
/// For structured env data (the `sync` block) that `set`'s scalar coercion +
/// unknown-name rejection can't handle. Not user-facing.
pub fn set_raw(command: &CommandContext, name: &str, value: Value) -> Result<()> {
    path_segments(name)?;
    if super::schema::is_structural(name) {
        bail!("env variable `{name}` is structural and read-only");
    }
    let mut map = load_map(command);
    map.insert(name.to_owned(), value);
    save_map(command, &map)
}

/// Every declared env variable plus every nested raw env value, in schema
/// order followed by recursively flattened JSON paths.
#[must_use]
pub fn resolve_all(command: &CommandContext) -> Vec<Resolved> {
    resolve_all_from(command, &load_map(command))
}

fn resolve_all_from(command: &CommandContext, map: &Map<String, Value>) -> Vec<Resolved> {
    let mut rows: Vec<Resolved> = VARS
        .iter()
        .map(|v| Resolved {
            name: v.name.to_owned(),
            value: resolve_one_from_map(command, map, v.name),
            description: v.description.to_owned(),
        })
        .collect();
    rows.extend(
        flatten_map(map)
            .into_iter()
            .filter(|(name, _)| !VARS.iter().any(|var| var.name == name))
            .map(|(name, value)| Resolved {
                value: if super::schema::is_sensitive(&name) {
                    Some("(set)".to_owned())
                } else {
                    value_to_string(&value)
                },
                name,
                description: "Nested value from env.json".to_owned(),
            }),
    );
    rows
}

fn resolve_one_from_map(
    command: &CommandContext,
    map: &Map<String, Value>,
    name: &str,
) -> Option<String> {
    if name == "root" {
        return Some(command.workspace.root().display().to_string());
    }
    let spec = VARS.iter().find(|spec| spec.name == name)?;
    map.get(name)
        .and_then(value_to_string)
        .or_else(|| {
            spec.legacy_config_fallback
                .then(|| legacy_config_value(command.workspace.as_ref(), name))
                .flatten()
        })
        .map(|value| {
            let value = spec
                .default
                .map_or_else(|| value.clone(), |default| trim_or_default(&value, default));
            if super::schema::is_sensitive(name) && !value.trim().is_empty() {
                "(set)".to_owned()
            } else {
                value
            }
        })
        .or_else(|| spec.default.map(str::to_owned))
}

fn parse_value(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| Value::from(raw))
}

fn path_segments(path: &str) -> Result<Vec<&str>> {
    let segments = path.split('.').collect::<Vec<_>>();
    if path.is_empty() || segments.iter().any(|segment| segment.is_empty()) {
        bail!("invalid env path `{path}`; use dot-separated names")
    }
    Ok(segments)
}

fn get_path<'a>(map: &'a Map<String, Value>, path: &str) -> Option<&'a Value> {
    let mut current: Option<&'a Value> = None;
    for segment in path_segments(path).ok()? {
        let value = match current {
            None => map.get(segment)?,
            Some(Value::Object(object)) => object.get(segment)?,
            Some(Value::Array(array)) => array.get(segment.parse::<usize>().ok()?)?,
            Some(_) => return None,
        };
        current = Some(value);
    }
    current
}

fn set_path(map: &mut Map<String, Value>, path: &str, value: Value) -> Result<()> {
    let segments = path_segments(path)?;
    let mut current = map;
    for segment in &segments[..segments.len() - 1] {
        let entry = current
            .entry((*segment).to_owned())
            .or_insert_with(|| Value::Object(Map::new()));
        current = entry.as_object_mut().ok_or_else(|| {
            anyhow::anyhow!("cannot descend through non-object env value `{segment}`")
        })?;
    }
    current.insert(segments[segments.len() - 1].to_owned(), value);
    Ok(())
}

fn flatten_map(map: &Map<String, Value>) -> Vec<(String, Value)> {
    let mut rows = Vec::new();
    for (name, value) in map {
        flatten_value(name, value, &mut rows);
    }
    rows
}

fn flatten_value(path: &str, value: &Value, rows: &mut Vec<(String, Value)>) {
    match value {
        Value::Object(object) if !object.is_empty() => {
            for (name, value) in object {
                flatten_value(&format!("{path}.{name}"), value, rows);
            }
        }
        Value::Array(array) if !array.is_empty() => {
            for (index, value) in array.iter().enumerate() {
                flatten_value(&format!("{path}.{index}"), value, rows);
            }
        }
        _ => rows.push((path.to_owned(), value.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::schema::{
        DEFAULT_CLAUDE_CMD, DEFAULT_CODEX_CMD, DEFAULT_OPENCODE_CMD, default_of,
    };

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
            crate::workspace::RegistryStore::from_path(std::path::PathBuf::from(
                "/missing/env.json",
            )),
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
}
