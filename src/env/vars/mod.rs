//! Reading and writing brain-env variables: get / set / resolve behind
//! `brain env`. Mirrors `settings::vars` but over the env store, and renders
//! into the shared `settings::Resolved` type. Nested JSON objects are exposed
//! as dot-separated paths so the full env store remains inspectable.

use std::path::Path;

use anyhow::{Result, bail};
use serde_json::{Map, Value};

use super::schema::{REDACTED, VARS, is_known, is_machine_global, known_names};
use super::store::{load_global_map, load_map, save_global_map, save_map};
use crate::settings::Resolved;
use crate::workspace::CommandContext;

pub(super) fn value_to_string(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

/// The raw explicit value for `name` (no default fallback).
///
/// Machine-global variables are read from the registry's top-level `env`, so
/// every workspace on this machine sees the same answer.
#[must_use]
pub fn get(command: &CommandContext, name: &str) -> Option<String> {
    let map = if is_machine_global(name) {
        load_global_map(command)
    } else {
        load_map(command)
    };
    if !name.contains('.') {
        return map.get(name).and_then(value_to_string);
    }
    get_path(&map, name).and_then(value_to_string)
}

/// The raw JSON value for `name` (no default fallback, no string coercion).
///
/// For structured env data — the `sync` block, the `skill_sessions` array —
/// whose readers want the JSON shape rather than [`get`]'s rendered string.
#[must_use]
pub fn get_raw(command: &CommandContext, name: &str) -> Option<Value> {
    let map = if is_machine_global(name) {
        load_global_map(command)
    } else {
        load_map(command)
    };
    if name.contains('.') {
        return get_path(&map, name).cloned();
    }
    map.get(name).cloned()
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
            .then(|| legacy_config_value(command.workspace.root(), name))
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

fn legacy_config_value(root: &Path, name: &str) -> Option<String> {
    crate::settings::load_map_at_root(root)
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
    if segments.len() == 1 && !is_known(name) {
        bail!("unknown env variable `{name}` (known: {})", known_names());
    }
    // A machine-scoped value is written once, to the registry's global map,
    // rather than into whichever workspace happened to be selected.
    if is_machine_global(name) {
        let mut global = load_global_map(command);
        set_path(&mut global, name, declared_value(name, value)?)?;
        return save_global_map(command, &global);
    }
    let mut map = load_map(command);
    if segments.len() > 1 {
        let top = segments[0];
        let top_value = map.get(top);
        let can_descend = top == "sync" || is_known(top) || top_value.is_some_and(Value::is_object);
        if !can_descend {
            bail!("unknown env object `{top}` (known: {})", known_names());
        }
    }
    set_path(&mut map, name, declared_value(name, value)?)?;
    save_map(command, &map)
}

/// Validate and canonicalize a declared scalar before it reaches the store.
///
/// Enum-valued variables are normalized here so every reader sees one spelling
/// and a typo fails at the CLI rather than silently degrading a later launch.
fn declared_value(name: &str, value: &str) -> Result<Value> {
    if name == crate::agent::default_frontend::ENV_VAR {
        return Ok(Value::from(crate::agent::default_frontend::canonicalize(
            value,
        )?));
    }
    Ok(parse_value(value))
}

/// Persist several declared scalar values to one selected workspace record.
pub(crate) fn set_many(command: &CommandContext, values: &[(&str, String)]) -> Result<()> {
    let mut map = load_map(command);
    for (name, value) in values {
        let segments = path_segments(name)?;
        if super::schema::is_structural(name) {
            bail!("env variable `{name}` is structural and read-only");
        }
        if segments.len() != 1 || !is_known(name) {
            bail!("unknown env variable `{name}` (known: {})", known_names());
        }
        set_path(&mut map, name, Value::String(value.clone()))?;
    }
    save_map(command, &map)
}

pub(crate) fn restore_values_if_unchanged(
    command: &CommandContext,
    before: &Map<String, Value>,
    written: &[(&str, String)],
) -> Result<()> {
    command
        .registry_store
        .transaction(|transaction| -> Result<()> {
            let mut registry = transaction.load()?;
            let selected = registry.select(Some(command.workspace.name().as_str()))?;
            anyhow::ensure!(
                selected.record().workspace_id == command.workspace.id(),
                "selected workspace identity changed before env rollback"
            );
            let canonical_name = selected.canonical_name().clone();
            let env = &mut registry
                .workspaces
                .get_mut(&canonical_name)
                .ok_or_else(|| anyhow::anyhow!("selected workspace record disappeared"))?
                .env;
            for (name, value) in written {
                if env.get(*name) != Some(&Value::String(value.clone())) {
                    continue;
                }
                if let Some(original) = before.get(*name) {
                    env.insert((*name).to_owned(), original.clone());
                } else {
                    env.remove(*name);
                }
            }
            transaction.save(&registry)?;
            Ok(())
        })
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
///
/// Machine-global rows are included with their effective value, so the
/// interactive `brain env set` picker offers every variable a user can set —
/// they just resolve from the registry's global map rather than this
/// workspace's record.
#[must_use]
pub fn resolve_all(command: &CommandContext) -> Vec<Resolved> {
    let mut rows = machine_global_rows(command);
    rows.extend(resolve_all_at(command.workspace.root(), &load_map(command)));
    rows
}

/// The declared machine-global rows, resolved from the registry's global map.
#[must_use]
pub(crate) fn machine_global_rows(command: &CommandContext) -> Vec<Resolved> {
    let global = load_global_map(command);
    VARS.iter()
        .filter(|spec| is_machine_global(spec.name))
        .map(|spec| Resolved {
            name: spec.name.to_owned(),
            value: global
                .get(spec.name)
                .and_then(value_to_string)
                .map(|value| {
                    if super::schema::is_sensitive(spec.name) && !value.trim().is_empty() {
                        REDACTED.to_owned()
                    } else {
                        value
                    }
                })
                .or_else(|| spec.default.map(str::to_owned)),
            description: spec.description.to_owned(),
        })
        .collect()
}

/// Every declared env row plus every nested raw value for the workspace rooted
/// at `root`, resolved from its own registry `env` map.
///
/// Root-based rather than selected-context-based so `brain env` can render one
/// block per registered workspace, not only the selected one.
#[must_use]
pub(crate) fn resolve_all_at(root: &Path, map: &Map<String, Value>) -> Vec<Resolved> {
    // Machine-global variables are not part of a workspace block: they would
    // render the same value under every workspace and invite the reader to
    // think each record holds its own.
    let mut rows: Vec<Resolved> = VARS
        .iter()
        .filter(|v| !is_machine_global(v.name))
        .map(|v| Resolved {
            name: v.name.to_owned(),
            value: resolve_one_at(root, map, v.name),
            description: v.description.to_owned(),
        })
        .collect();
    rows.extend(
        flatten_map(map)
            .into_iter()
            .filter(|(name, _)| !VARS.iter().any(|var| var.name == name))
            .map(|(name, value)| Resolved {
                value: if super::schema::is_sensitive(&name) {
                    Some(REDACTED.to_owned())
                } else {
                    value_to_string(&value)
                },
                name,
                description: "Nested value from env.json".to_owned(),
            }),
    );
    rows
}

fn resolve_one_at(root: &Path, map: &Map<String, Value>, name: &str) -> Option<String> {
    if name == "root" {
        return Some(root.display().to_string());
    }
    let spec = VARS.iter().find(|spec| spec.name == name)?;
    map.get(name)
        .and_then(value_to_string)
        .or_else(|| {
            spec.legacy_config_fallback
                .then(|| legacy_config_value(root, name))
                .flatten()
        })
        .map(|value| {
            let value = spec
                .default
                .map_or_else(|| value.clone(), |default| trim_or_default(&value, default));
            if super::schema::is_sensitive(name) && !value.trim().is_empty() {
                REDACTED.to_owned()
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

/// Write `value` at a dotted env path, creating missing intermediate objects.
///
/// Descends through objects by name and through arrays by index, mirroring
/// [`get_path`], so one element of a structured list (`skill_sessions.0.prompt`)
/// is addressable like any nested field. Missing object keys are created; a
/// missing *array* index is an error rather than a silently invented entry.
fn set_path(map: &mut Map<String, Value>, path: &str, value: Value) -> Result<()> {
    let segments = path_segments(path)?;
    let (last, parents) = segments.split_last().expect("path has a segment");
    let mut current = map
        .entry((*parents.first().unwrap_or(last)).to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    if parents.is_empty() {
        map.insert((*last).to_owned(), value);
        return Ok(());
    }
    for (depth, segment) in parents.iter().enumerate().skip(1) {
        current = descend(current, segment, &segments[..=depth])?;
    }
    match current {
        Value::Object(object) => {
            object.insert((*last).to_owned(), value);
            Ok(())
        }
        Value::Array(array) => {
            let index = array_index(last, array.len(), &segments)?;
            array[index] = value;
            Ok(())
        }
        _ => Err(anyhow::anyhow!(
            "cannot descend through non-object env value `{}`",
            parents.join(".")
        )),
    }
}

/// One step of [`set_path`]'s walk, creating a missing object key. `walked` is
/// the path so far, for an error a user can locate.
fn descend<'a>(current: &'a mut Value, segment: &str, walked: &[&str]) -> Result<&'a mut Value> {
    match current {
        Value::Array(array) => {
            let index = array_index(segment, array.len(), walked)?;
            Ok(&mut array[index])
        }
        Value::Object(object) => Ok(object
            .entry(segment.to_owned())
            .or_insert_with(|| Value::Object(Map::new()))),
        _ => Err(anyhow::anyhow!(
            "cannot descend through non-object env value `{segment}`"
        )),
    }
}

fn array_index(segment: &str, len: usize, walked: &[&str]) -> Result<usize> {
    segment
        .parse::<usize>()
        .ok()
        .filter(|index| *index < len)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no env array element at `{}`; the list holds {len} entr{}",
                walked.join("."),
                if len == 1 { "y" } else { "ies" }
            )
        })
}

pub(super) fn flatten_map(map: &Map<String, Value>) -> Vec<(String, Value)> {
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
mod tests;
