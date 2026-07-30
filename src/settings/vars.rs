//! Reading and writing individual variables: name canonicalization, the
//! JSON↔string coercion typed readers rely on, and the get / set / resolve
//! operations behind `brain config`.

use anyhow::{Result, bail};
use serde_json::{Map, Value};

use super::schema::{Resolved, VARS, default_of, is_known, known_names};
use super::store::{load_map, save_map};

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
pub fn get(name: &str) -> Option<String> {
    load_map()
        .get(name)
        .and_then(|value| value_to_string(name, value))
}

/// The effective value for a known variable: explicit override else default.
#[must_use]
pub fn resolve_one(name: &str) -> Option<String> {
    if !is_known(name) {
        return None;
    }
    get(name).or_else(|| default_of(name).map(str::to_owned))
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
pub fn set(name: &str, value: &str) -> Result<()> {
    if !is_known(name) {
        bail!(
            "unknown config variable `{name}` (known: {})",
            known_names()
        );
    }
    let mut map = load_map();
    map.insert(name.to_owned(), parse_value(value));
    save_map(&map)
}

/// Every declared variable with its resolved value, in schema order.
#[must_use]
pub fn resolve_all() -> Vec<Resolved> {
    resolve_all_from(&load_map())
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
        assert!(set("root", "/srv/brain").is_err());
    }

    #[test]
    fn markdown_to_pdf_path_is_no_longer_a_brain_config_variable() {
        // It moved to brain env; `brain config` must reject it.
        assert!(
            resolve_all_from(&Map::new())
                .iter()
                .all(|r| r.name != "markdown_to_pdf_path")
        );
        assert!(set("markdown_to_pdf_path", "/x").is_err());
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
        assert!(set("claude_cmd", "claude").is_err());
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
}
