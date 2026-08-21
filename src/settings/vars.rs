//! Reading and writing individual variables: name canonicalization, the
//! JSON↔string coercion typed readers rely on, and the get / set / resolve
//! operations behind `brain config`.

use anyhow::{Result, bail};
use serde_json::{Map, Value};

use super::schema::{Resolved, VARS, default_of, is_known, known_names};
use super::store::{load_map, save_map};
use crate::users::Users;
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

/// The effective value for a known variable: live portable roster for the
/// variables it superseded, else explicit override, else default.
#[must_use]
pub fn resolve_one(workspace: &WorkspaceContext, name: &str) -> Option<String> {
    resolve_one_with(
        &load_map(workspace),
        portable_users(workspace).as_ref(),
        name,
    )
}

/// Pure core of [`resolve_one`].
pub(super) fn resolve_one_with(
    map: &Map<String, Value>,
    users: Option<&Users>,
    name: &str,
) -> Option<String> {
    if !is_known(name) {
        return None;
    }
    live_value(name, users)
        .or_else(|| map.get(name).and_then(|value| value_to_string(name, value)))
        .or_else(|| default_of(name).map(str::to_owned))
}

/// The portable roster's answer for a superseded variable, if it has one. Pure.
fn live_value(name: &str, users: Option<&Users>) -> Option<String> {
    users.and_then(|users| super::portable::active_value(name, users))
}

/// The workspace's portable roster, or `None` when it cannot be read.
///
/// An unreadable roster must never take the table down: `brain config` is
/// exactly where a user looks when a workspace is half-configured.
fn portable_users(workspace: &WorkspaceContext) -> Option<Users> {
    crate::users::UsersStore::load_from(&crate::users::UsersStore::path(workspace)).ok()
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

fn parse_setting_value(name: &str, raw: &str) -> Result<Value> {
    if matches!(name, "allowed_mcps" | "allowed_skills") {
        let names = if raw.trim_start().starts_with('[') {
            serde_json::from_str::<Vec<String>>(raw).map_err(|error| {
                anyhow::anyhow!("{name} must be a JSON array or comma-separated names: {error}")
            })?
        } else {
            raw.split(',')
                .map(str::trim)
                .filter(|entry| !entry.is_empty())
                .map(str::to_owned)
                .collect()
        };
        return Ok(Value::Array(names.into_iter().map(Value::from).collect()));
    }
    Ok(parse_value(raw))
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
    // Writing here would persist a value nothing enforces, which is the same
    // silent no-op that made a configured receiver look unconfigured.
    if super::portable::is_superseded(name) {
        bail!(super::portable::refusal(name, workspace.name().as_str()));
    }
    if name == "enable_daily_triage_check" && !matches!(value.trim(), "true" | "false") {
        bail!("enable_daily_triage_check must be true or false");
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
    map.insert(name.to_owned(), parse_setting_value(name, value)?);
    save_map(workspace, &map, &owner)
}

/// Every declared variable with its resolved value, in schema order.
#[must_use]
pub fn resolve_all(workspace: &WorkspaceContext) -> Vec<Resolved> {
    resolve_all_with(&load_map(workspace), portable_users(workspace).as_ref())
}

/// Pure core of [`resolve_all`]: resolve against an explicit map so the schema
/// and default logic are testable without touching the real store.
#[cfg(test)]
pub(super) fn resolve_all_from(map: &Map<String, Value>) -> Vec<Resolved> {
    resolve_all_with(map, None)
}

/// Pure core of [`resolve_all`], with the portable roster that outranks the
/// config store for the variables it superseded.
pub(super) fn resolve_all_with(map: &Map<String, Value>, users: Option<&Users>) -> Vec<Resolved> {
    VARS.iter()
        .map(|v| Resolved {
            name: v.name.to_owned(),
            value: live_value(v.name, users)
                .or_else(|| {
                    map.get(v.name)
                        .and_then(|value| value_to_string(v.name, value))
                })
                .or_else(|| v.default.map(str::to_owned)),
            description: v.description.to_owned(),
        })
        .collect()
}

#[cfg(test)]
mod tests;
