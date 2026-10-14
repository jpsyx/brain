//! Pure cleanup of already-scanned project metadata before-images.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::model::Problem;
use super::snapshot::ProjectSnapshot;

/// Return only after-images; the snapshot remains the source of before-images.
pub(crate) fn clean(
    projects: &BTreeMap<String, ProjectSnapshot>,
    removed: &BTreeSet<String>,
) -> Result<BTreeMap<String, Vec<u8>>, Problem> {
    let mut after = BTreeMap::new();
    for (path, project) in projects {
        let mut value: Value =
            serde_json::from_slice(&project.before).map_err(|error| invalid(path, error))?;
        let object = value
            .as_object_mut()
            .ok_or_else(|| invalid(path, "metadata must be an object"))?;
        let Some(tasks) = object.get_mut("tasks") else {
            continue;
        };
        let tasks = tasks
            .as_array_mut()
            .ok_or_else(|| invalid(path, "tasks must be an array of strings"))?;
        if tasks.iter().any(|item| !item.is_string()) {
            return Err(invalid(path, "tasks must be an array of strings"));
        }
        let before_len = tasks.len();
        tasks.retain(|item| item.as_str().is_some_and(|id| !removed.contains(id)));
        if tasks.len() == before_len {
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
