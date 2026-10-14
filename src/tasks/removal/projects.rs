//! Pure cleanup of already-scanned project metadata before-images.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::model::Problem;
use super::snapshot::ProjectSnapshot;

/// Return only after-images; the snapshot remains the source of before-images.
#[cfg(test)]
pub(crate) fn clean(
    projects: &BTreeMap<String, ProjectSnapshot>,
    removed: &BTreeSet<String>,
) -> Result<BTreeMap<String, Vec<u8>>, Problem> {
    compose(projects, removed, &BTreeMap::new(), &BTreeMap::new())
}

pub(super) fn compose(
    projects: &BTreeMap<String, ProjectSnapshot>,
    removed: &BTreeSet<String>,
    replacements: &BTreeMap<String, String>,
    history: &BTreeMap<String, Vec<Value>>,
) -> Result<BTreeMap<String, Vec<u8>>, Problem> {
    let mut after = BTreeMap::new();
    for (path, project) in projects {
        let mut value: Value =
            serde_json::from_slice(&project.before).map_err(|error| invalid(path, error))?;
        let object = value
            .as_object_mut()
            .ok_or_else(|| invalid(path, "metadata must be an object"))?;
        let original = object.clone();
        if let Some(tasks) = object.get_mut("tasks") {
            let tasks = tasks
                .as_array_mut()
                .ok_or_else(|| invalid(path, "tasks must be an array of strings"))?;
            let strings = tasks
                .iter()
                .map(|item| {
                    item.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| invalid(path, "tasks must be an array of strings"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let cleaned = if replacements.is_empty() {
                strings
                    .into_iter()
                    .filter(|id| !removed.contains(id))
                    .collect()
            } else {
                super::references::replace_tokens(&strings, replacements, false)
            };
            *tasks = cleaned.into_iter().map(Value::String).collect();
        }
        if let Some(entries) = history.get(path) {
            let existing = object
                .entry("deleted_backlog_tasks")
                .or_insert_with(|| Value::Array(Vec::new()))
                .as_array_mut()
                .ok_or_else(|| invalid(path, "deleted_backlog_tasks must be an array"))?;
            existing.extend(entries.iter().cloned());
        }
        if *object == original {
            continue;
        }
        let mut bytes = serde_json::to_vec_pretty(&value).map_err(|error| invalid(path, error))?;
        bytes.push(b'\n');
        after.insert(path.clone(), bytes);
    }
    Ok(after)
}

fn invalid(path: &str, error: impl std::fmt::Display) -> Problem {
    Problem::new(
        "invalid_project_metadata",
        &error.to_string(),
        serde_json::json!({"path": path}),
    )
}
