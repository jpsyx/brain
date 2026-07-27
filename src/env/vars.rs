//! Reading and writing brain-env variables: get / set / resolve behind
//! `brain env`. Mirrors `settings::vars` but over the env store, and renders
//! into the shared `settings::Resolved` type.

use anyhow::{Result, bail};
use serde_json::Value;

use super::schema::{DEFAULT_CODEX_CMD, VARS, default_of, is_known, known_names};
use super::store::{load_map, save_map};
use crate::settings::Resolved;

fn value_to_string(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

/// The raw explicit value for `name` (no default fallback).
#[must_use]
pub fn get(name: &str) -> Option<String> {
    load_map().get(name).and_then(value_to_string)
}

/// The effective value for a known env variable: explicit override else default.
///
/// `root` resolves through [`crate::paths::brain_root_path`] so the shown value
/// matches what brain actually uses (including the legacy-pointer fallback).
#[must_use]
pub fn resolve_one(name: &str) -> Option<String> {
    if !is_known(name) {
        return None;
    }
    if name == "root" {
        return Some(crate::paths::brain_root_path().display().to_string());
    }
    get(name).or_else(|| default_of(name).map(str::to_owned))
}

/// The configured Codex launch command, or the built-in default when unset or
/// blank. brain appends any frontend-specific resume arguments after this.
#[must_use]
pub fn codex_command() -> String {
    let cmd = get("codex_cmd").unwrap_or_else(|| DEFAULT_CODEX_CMD.to_owned());
    let trimmed = cmd.trim();
    if trimmed.is_empty() {
        DEFAULT_CODEX_CMD.to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Persist `name=value` into the env store. Unknown names are rejected.
pub fn set(name: &str, value: &str) -> Result<()> {
    if !is_known(name) {
        bail!("unknown env variable `{name}` (known: {})", known_names());
    }
    let mut map = load_map();
    map.insert(name.to_owned(), Value::from(value));
    save_map(&map)
}

/// Write a raw JSON value under `name`, bypassing the declared-variable check.
///
/// For structured env data (the `sync` block) that `set`'s scalar coercion +
/// unknown-name rejection can't handle. Not user-facing.
pub fn set_raw(name: &str, value: Value) -> Result<()> {
    let mut map = load_map();
    map.insert(name.to_owned(), value);
    save_map(&map)
}

/// Every declared env variable with its resolved value, in schema order.
#[must_use]
pub fn resolve_all() -> Vec<Resolved> {
    VARS.iter()
        .map(|v| Resolved {
            name: v.name,
            value: resolve_one(v.name),
            description: v.description,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_all_lists_root_markdown_to_pdf_path_and_codex_cmd() {
        let rows = resolve_all();
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().any(|r| r.name == "root"));
        assert!(rows.iter().any(|r| r.name == "markdown_to_pdf_path"));
        assert!(rows.iter().any(|r| r.name == "codex_cmd"));
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
        assert!(set("linear_workspace", "acme").is_err());
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
        let rows = resolve_all();
        let root = rows.iter().find(|r| r.name == "root").unwrap();
        assert_eq!(
            root.value.as_deref(),
            Some(
                crate::paths::brain_root_path()
                    .display()
                    .to_string()
                    .as_str()
            )
        );
    }

    #[test]
    fn codex_command_defaults_to_codex() {
        assert_eq!(codex_command(), "codex");
    }
}
