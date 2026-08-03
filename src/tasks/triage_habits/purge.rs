//! Managed-triage purge decisions and derived-reference rewriting.

use std::collections::BTreeSet;
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
    pub(super) fn collect(csv: &CsvFile) -> Self {
        let mut values = BTreeSet::new();
        for row in &csv.rows {
            if is_managed_system_key(&field(row, "system_key")) {
                values.insert(field(row, "task_uuid"));
                values.insert(field(row, "task_id"));
            }
        }
        values.retain(|value| !value.trim().is_empty());
        Self { values }
    }

    pub(super) fn extend(&mut self, other: Self) {
        self.values.extend(other.values);
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
    collect_metadata(root.join("projects"), &mut paths);
    collect_task_indexes(root.join("tasks"), &mut paths);
    let mut changes = Vec::new();
    for path in paths {
        let before = std::fs::read(&path)
            .with_context(|| format!("reading derived task index {}", path.display()))?;
        let after = if path.file_name().and_then(|name| name.to_str()) == Some(".METADATA.json") {
            purge_json(&before, identities).unwrap_or_else(|| before.clone())
        } else {
            purge_text(&before, identities)
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

fn collect_metadata(root: PathBuf, paths: &mut Vec<PathBuf>) {
    if !root.exists() {
        return;
    }
    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
    {
        if entry.file_type().is_file() && entry.file_name() == ".METADATA.json" {
            paths.push(entry.into_path());
        }
    }
}

fn collect_task_indexes(root: PathBuf, paths: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if path.is_file()
            && !path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("csv"))
            && (name.contains("agenda") || name.contains("index") || name.contains("lookup"))
        {
            paths.push(path);
        }
    }
}

fn purge_json(bytes: &[u8], identities: &ManagedIdentities) -> Option<Vec<u8>> {
    let mut value = serde_json::from_slice::<serde_json::Value>(bytes).ok()?;
    purge_json_value(&mut value, identities);
    let mut output = serde_json::to_vec_pretty(&value).ok()?;
    output.push(b'\n');
    Some(output)
}

fn purge_json_value(value: &mut serde_json::Value, identities: &ManagedIdentities) {
    match value {
        serde_json::Value::Array(values) => {
            values.retain(|value| {
                !value
                    .as_str()
                    .is_some_and(|text| identities.values.contains(text))
            });
            for value in values {
                purge_json_value(value, identities);
            }
        }
        serde_json::Value::Object(map) => {
            for value in map.values_mut() {
                purge_json_value(value, identities);
            }
        }
        _ => {}
    }
}

fn purge_text(bytes: &[u8], identities: &ManagedIdentities) -> Vec<u8> {
    let text = String::from_utf8_lossy(bytes);
    let trailing_newline = text.ends_with('\n');
    let mut lines = text
        .lines()
        .filter(|line| {
            !identities
                .values
                .iter()
                .any(|identity| line_contains_identity(line, identity))
        })
        .collect::<Vec<_>>()
        .join("\n");
    if trailing_newline && !lines.is_empty() {
        lines.push('\n');
    }
    lines.into_bytes()
}

fn line_contains_identity(line: &str, identity: &str) -> bool {
    line.split(|character: char| {
        !character.is_ascii_alphanumeric() && character != '-' && character != '_'
    })
    .any(|token| token == identity)
}
