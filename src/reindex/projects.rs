//! Pure `projects-lookup.csv` row derivation from a project `.METADATA.json`.

use serde::Deserialize;

/// The `projects-lookup.csv` column order.
pub const PROJECTS_HEADER: [&str; 7] = [
    "name",
    "namespace",
    "title",
    "status",
    "priority",
    "due",
    "directory",
];

/// The subset of a project `.METADATA.json` that feeds the lookup CSV. Extra
/// keys (e.g. `tasks`, `directory`) are ignored; the authoritative `directory`
/// is the filesystem path, passed in separately.
#[derive(Debug, Default, Deserialize)]
pub struct ProjectMeta {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub namespace: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub priority: String,
    #[serde(default)]
    pub due: String,
}

/// Parse a project `.METADATA.json`; `None` if it isn't valid JSON.
#[must_use]
pub fn parse_project_meta(json: &str) -> Option<ProjectMeta> {
    serde_json::from_str(json).ok()
}

/// Build one lookup row. `directory` is the brain-root-relative path (the
/// authoritative source, so a renamed folder is reflected without editing JSON).
#[must_use]
pub fn project_row(meta: &ProjectMeta, directory: &str) -> Vec<String> {
    let due = if meta.due.trim().is_empty() {
        "none".to_owned()
    } else {
        meta.due.clone()
    };
    vec![
        meta.name.clone(),
        meta.namespace.clone(),
        meta.title.clone(),
        meta.status.clone(),
        meta.priority.clone(),
        due,
        directory.to_owned(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_metadata_fields_directly_and_uses_filesystem_directory() {
        let json = r#"{
            "name": "avandar__finalize-investor-agreement-terms",
            "namespace": "avandar",
            "title": "Finalize Avandar investor agreement terms",
            "status": "in-progress",
            "priority": "p1",
            "due": "2026-07-31",
            "directory": "projects/STALE-directory-from-json",
            "tasks": []
        }"#;
        let meta = parse_project_meta(json).expect("valid json");
        let row = project_row(&meta, "projects/avandar__finalize-investor-agreement-terms");
        assert_eq!(
            row,
            vec![
                "avandar__finalize-investor-agreement-terms",
                "avandar",
                "Finalize Avandar investor agreement terms",
                "in-progress",
                "p1",
                "2026-07-31",
                // filesystem path wins over the stale `directory` in the JSON
                "projects/avandar__finalize-investor-agreement-terms",
            ]
        );
    }

    #[test]
    fn missing_due_becomes_none() {
        let meta = parse_project_meta(
            r#"{"name":"x","namespace":"n","title":"t","status":"in-progress","priority":"p2"}"#,
        )
        .expect("valid json");
        let row = project_row(&meta, "projects/x");
        assert_eq!(row[5], "none");
    }
}
