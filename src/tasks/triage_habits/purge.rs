//! Managed-triage purge decisions and derived-reference rewriting.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::model::is_managed_system_key;
use super::transaction::FileChange;
use crate::tasks::complete::{CsvFile, field};

#[derive(Default)]
pub(super) struct ManagedIdentities {
    values: BTreeSet<String>,
}

impl ManagedIdentities {
    pub(super) fn collect_all(csvs: &[&CsvFile]) -> Self {
        let mut values = BTreeSet::new();
        let mut display_counts = BTreeMap::<String, usize>::new();
        let mut managed_displays = BTreeSet::new();
        for csv in csvs {
            for row in &csv.rows {
                let display = field(row, "task_id");
                if !display.trim().is_empty() {
                    *display_counts.entry(display.clone()).or_default() += 1;
                }
                if is_managed_system_key(&field(row, "system_key")) {
                    values.insert(field(row, "task_uuid"));
                    managed_displays.insert(display);
                }
            }
        }
        values.extend(
            managed_displays
                .into_iter()
                .filter(|display| display_counts.get(display).copied() == Some(1)),
        );
        values.retain(|value| !value.trim().is_empty());
        Self { values }
    }
}

pub(super) fn purge_rows(csv: &mut CsvFile) {
    csv.rows
        .retain(|row| !is_managed_system_key(&field(row, "system_key")));
}

pub(super) fn derived_changes(
    root: &Path,
    identities: &ManagedIdentities,
) -> Result<Vec<FileChange>> {
    let mut paths = Vec::new();
    collect_metadata(root.join("projects"), &mut paths)?;
    collect_task_indexes(&root.join("tasks"), &mut paths)?;
    let mut changes = Vec::new();
    for path in paths {
        let before = std::fs::read(&path)
            .with_context(|| format!("reading derived task index {}", path.display()))?;
        let after = if path.file_name().and_then(|name| name.to_str()) == Some(".METADATA.json") {
            purge_json(&before, identities)
                .with_context(|| format!("parsing project metadata {}", path.display()))?
        } else {
            purge_text(&before, identities)
                .with_context(|| format!("parsing derived task index {}", path.display()))?
        };
        if after != before {
            changes.push(FileChange {
                path,
                before: Some(before),
                after,
            });
        }
    }
    Ok(changes)
}

fn collect_metadata(root: PathBuf, paths: &mut Vec<PathBuf>) -> Result<()> {
    match std::fs::metadata(&root) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).with_context(|| format!("inspecting {}", root.display())),
    }
    for entry in walkdir::WalkDir::new(root) {
        let entry = entry.with_context(|| "walking project metadata")?;
        if entry.file_type().is_file() && entry.file_name() == ".METADATA.json" {
            paths.push(entry.into_path());
        }
    }
    Ok(())
}

fn collect_task_indexes(root: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).with_context(|| format!("reading {}", root.display())),
    };
    for entry in entries {
        let entry = entry.with_context(|| format!("reading entry in {}", root.display()))?;
        let path = entry.path();
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| anyhow::anyhow!("non-UTF-8 task index name"))?
            .to_ascii_lowercase();
        if entry
            .file_type()
            .with_context(|| format!("inspecting {}", path.display()))?
            .is_file()
            && !path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("csv"))
            && (name.contains("agenda") || name.contains("index") || name.contains("lookup"))
        {
            paths.push(path);
        }
    }
    Ok(())
}

fn purge_json(bytes: &[u8], identities: &ManagedIdentities) -> Result<Vec<u8>> {
    let mut value = serde_json::from_slice::<serde_json::Value>(bytes)?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("project metadata must be an object"))?;
    if let Some(tasks) = object.get_mut("tasks") {
        let tasks = tasks
            .as_array_mut()
            .ok_or_else(|| anyhow::anyhow!("project metadata tasks must be an array"))?;
        tasks.retain(|value| {
            !value
                .as_str()
                .is_some_and(|text| identities.values.contains(text))
        });
    }
    let mut output = serde_json::to_vec_pretty(&value)?;
    output.push(b'\n');
    Ok(output)
}

fn purge_text(bytes: &[u8], identities: &ManagedIdentities) -> Result<Vec<u8>> {
    let mut text = std::str::from_utf8(bytes)?.to_owned();
    for identity in &identities.values {
        let mut output = String::with_capacity(text.len());
        let mut remainder = text.as_str();
        while let Some(offset) = remainder.find(identity) {
            let (before, at) = remainder.split_at(offset);
            let after = &at[identity.len()..];
            output.push_str(before);
            if bounded(before.chars().next_back()) || bounded(after.chars().next()) {
                output.push_str(identity);
            }
            remainder = after;
        }
        output.push_str(remainder);
        text = output;
    }
    Ok(text.into_bytes())
}

fn bounded(character: Option<char>) -> bool {
    character.is_some_and(|character| {
        character.is_ascii_alphanumeric() || character == '-' || character == '_'
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identities(values: &[&str]) -> ManagedIdentities {
        ManagedIdentities {
            values: values.iter().map(|value| (*value).to_owned()).collect(),
        }
    }

    fn csv(rows: &[(&str, &str, &str)]) -> CsvFile {
        CsvFile {
            header: vec!["task_uuid".into(), "task_id".into(), "system_key".into()],
            rows: rows
                .iter()
                .map(|(task_uuid, task_id, system_key)| {
                    [
                        ("task_uuid".to_owned(), (*task_uuid).to_owned()),
                        ("task_id".to_owned(), (*task_id).to_owned()),
                        ("system_key".to_owned(), (*system_key).to_owned()),
                    ]
                    .into_iter()
                    .collect()
                })
                .collect(),
        }
    }

    #[test]
    fn ambiguous_display_id_is_not_purged_when_uuid_identity_is_available() {
        let tasks = csv(&[("ordinary-uuid", "H1", "")]);
        let habits = csv(&[("managed-uuid", "H1", "brain.triage.daily")]);

        let identities = ManagedIdentities::collect_all(&[&tasks, &habits]);

        assert!(identities.values.contains("managed-uuid"));
        assert!(!identities.values.contains("H1"));
    }

    #[test]
    fn metadata_purge_changes_only_the_schema_defined_tasks_field() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("projects/alpha");
        std::fs::create_dir_all(&project).unwrap();
        let path = project.join(".METADATA.json");
        std::fs::write(
            &path,
            br#"{"tasks":["managed-uuid","keep"],"tags":["managed-uuid"],"nested":{"tasks":["managed-uuid"]}}"#,
        )
        .unwrap();

        let changes = derived_changes(root.path(), &identities(&["managed-uuid"])).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&changes[0].after).unwrap();

        assert_eq!(value["tasks"], serde_json::json!(["keep"]));
        assert_eq!(value["tags"], serde_json::json!(["managed-uuid"]));
        assert_eq!(
            value["nested"]["tasks"],
            serde_json::json!(["managed-uuid"])
        );
    }

    #[test]
    fn malformed_metadata_aborts_the_purge_transaction() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("projects/alpha");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join(".METADATA.json"), b"not json\n").unwrap();

        assert!(derived_changes(root.path(), &identities(&["managed-uuid"])).is_err());
    }

    #[test]
    fn invalid_utf8_task_index_aborts_the_purge_transaction() {
        let root = tempfile::tempdir().unwrap();
        let tasks = root.path().join("tasks");
        std::fs::create_dir_all(&tasks).unwrap();
        std::fs::write(tasks.join("agenda-index.md"), [0xff, b'\n']).unwrap();

        assert!(derived_changes(root.path(), &identities(&["managed-uuid"])).is_err());
    }

    #[test]
    fn mixed_index_lines_preserve_unrelated_bytes() {
        let root = tempfile::tempdir().unwrap();
        let tasks = root.path().join("tasks");
        std::fs::create_dir_all(&tasks).unwrap();
        std::fs::write(
            tasks.join("agenda-index.md"),
            b"before managed-uuid after\nkeep\n",
        )
        .unwrap();

        let changes = derived_changes(root.path(), &identities(&["managed-uuid"])).unwrap();

        assert_eq!(changes[0].after, b"before  after\nkeep\n");
    }

    #[cfg(unix)]
    #[test]
    fn metadata_traversal_error_aborts_the_purge_transaction() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let projects = root.path().join("projects");
        let blocked = projects.join("blocked");
        std::fs::create_dir_all(&blocked).unwrap();
        std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o000)).unwrap();

        let result = derived_changes(root.path(), &identities(&["managed-uuid"]));

        std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result.is_err());
    }
}
