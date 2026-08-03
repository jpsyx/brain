//! Inactive portable task-schema migration primitives.
//!
//! This module performs no discovery and is not called by bootstrap, task
//! commands, readiness, or sync. The coordinated rollout supplies an explicit
//! legacy-sync precondition, workspace root, and machine-local backup path.

use std::collections::{BTreeMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Map, Value, json};

use super::identity::{CsvKind, TaskUuid, legacy_task_uuid};
use crate::workspace::WorkspaceId;

pub const TASK_SCHEMA_VERSION: u64 = 2;

const PORTABLE_FILES: [&str; 5] = [
    "tasks.csv",
    "habits.csv",
    ".tasks_next_id",
    ".habits_next_id",
    "SCHEMA.json",
];

/// Rollout-owned status of the required last legacy semantic sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacySemanticSync {
    /// Sync is configured and the rollout has not completed its final legacy pass.
    Required,
    /// The rollout completed and journaled the final legacy semantic sync.
    Complete,
    /// This workspace has no configured sync transport.
    NotConfigured,
}

/// All explicit capabilities needed to invoke the otherwise inactive helper.
#[derive(Debug, Clone, Copy)]
pub struct TaskSchemaMigration<'a> {
    pub workspace_id: WorkspaceId,
    pub workspace_root: &'a Path,
    pub backup_dir: &'a Path,
    pub legacy_semantic_sync: LegacySemanticSync,
}

/// Whether an inactive migration changed its fixture/workspace inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationOutcome {
    Migrated,
    AlreadyCurrent,
}

/// Apply the schema conversion only after a rollout coordinator supplies the
/// legacy-sync decision and a machine-local backup destination.
pub fn migrate_inactive(request: TaskSchemaMigration<'_>) -> Result<MigrationOutcome> {
    let tasks_dir = request.workspace_root.join("tasks");
    let tasks_path = tasks_dir.join("tasks.csv");
    let habits_path = tasks_dir.join("habits.csv");
    let schema_path = tasks_dir.join("SCHEMA.json");
    let tasks_bytes = read_required(&tasks_path)?;
    let habits_bytes = read_required(&habits_path)?;
    let schema_bytes = read_required(&schema_path)?;

    if is_current(&tasks_bytes, &habits_bytes, &schema_bytes)? {
        return Ok(MigrationOutcome::AlreadyCurrent);
    }
    if request.legacy_semantic_sync == LegacySemanticSync::Required {
        bail!(
            "legacy semantic sync must be completed by the coordinated rollout before task UUID migration"
        );
    }

    back_up_portable_files(&tasks_dir, request.backup_dir)?;
    let migrated_tasks = migrate_csv(&tasks_bytes, request.workspace_id, CsvKind::Tasks)?;
    let migrated_habits = migrate_csv(&habits_bytes, request.workspace_id, CsvKind::Habits)?;
    let migrated_schema = migrate_schema_metadata(&schema_bytes)?;

    let staged = [
        stage(&tasks_path, &migrated_tasks)?,
        stage(&habits_path, &migrated_habits)?,
        stage(&schema_path, &migrated_schema)?,
    ];
    for (temporary, destination) in staged {
        fs::rename(&temporary, &destination).with_context(|| {
            format!(
                "atomically replacing {} from {}",
                destination.display(),
                temporary.display()
            )
        })?;
        sync_parent(&destination);
    }
    Ok(MigrationOutcome::Migrated)
}

fn read_required(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).with_context(|| format!("reading required task schema input {}", path.display()))
}

fn is_current(tasks: &[u8], habits: &[u8], schema: &[u8]) -> Result<bool> {
    let schema: Value = serde_json::from_slice(schema).context("parsing tasks/SCHEMA.json")?;
    Ok(
        schema.get("task_schema_version").and_then(Value::as_u64) == Some(TASK_SCHEMA_VERSION)
            && csv_has_current_identity(tasks)?
            && csv_has_current_identity(habits)?,
    )
}

fn csv_has_current_identity(bytes: &[u8]) -> Result<bool> {
    let mut reader = csv::ReaderBuilder::new().flexible(true).from_reader(bytes);
    let headers = reader.headers()?.clone();
    if headers.get(0) != Some("task_uuid")
        || !headers.iter().any(|column| column == "task_id")
        || !headers.iter().any(|column| column == "assigned_to")
        || headers.iter().any(|column| column == "assignee")
    {
        return Ok(false);
    }
    let Some(uuid_index) = headers.iter().position(|column| column == "task_uuid") else {
        return Ok(false);
    };
    for record in reader.records() {
        let record = record?;
        if TaskUuid::parse(record.get(uuid_index).unwrap_or_default()).is_err() {
            return Ok(false);
        }
    }
    Ok(true)
}

fn back_up_portable_files(tasks_dir: &Path, backup_dir: &Path) -> Result<()> {
    let destination_dir = backup_dir.join("tasks");
    fs::create_dir_all(&destination_dir).with_context(|| {
        format!(
            "creating task migration backup {}",
            destination_dir.display()
        )
    })?;
    for name in PORTABLE_FILES {
        let source = tasks_dir.join(name);
        if !source.exists() {
            continue;
        }
        let bytes = fs::read(&source)
            .with_context(|| format!("reading task migration backup input {}", source.display()))?;
        let destination = destination_dir.join(name);
        if destination.exists() {
            let existing = fs::read(&destination).with_context(|| {
                format!(
                    "reading existing task migration backup {}",
                    destination.display()
                )
            })?;
            if existing != bytes {
                bail!(
                    "task migration backup already exists with different bytes: {}",
                    destination.display()
                );
            }
            continue;
        }
        write_new(&destination, &bytes)?;
    }
    sync_parent(&destination_dir);
    Ok(())
}

fn migrate_csv(bytes: &[u8], workspace_id: WorkspaceId, kind: CsvKind) -> Result<Vec<u8>> {
    let mut reader = csv::ReaderBuilder::new().flexible(true).from_reader(bytes);
    let source_header = reader
        .headers()?
        .iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if !source_header.iter().any(|column| column == "task_id") {
        bail!("{} CSV is missing task_id", kind.as_str());
    }
    reject_duplicate_columns(&source_header, kind)?;
    let header = migrated_header(&source_header);
    let has_assigned_to = source_header.iter().any(|column| column == "assigned_to");
    let mut rows = Vec::new();
    for (index, record) in reader.records().enumerate() {
        let record =
            record.with_context(|| format!("parsing {} row {}", kind.as_str(), index + 2))?;
        let mut row = source_header
            .iter()
            .enumerate()
            .map(|(column_index, column)| {
                (
                    column.clone(),
                    record.get(column_index).unwrap_or_default().to_owned(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let display_id = row.get("task_id").map_or("", String::as_str).trim();
        if display_id.is_empty() {
            bail!("{} row {} has an empty task_id", kind.as_str(), index + 2);
        }
        let task_uuid = match row.get("task_uuid").map(String::as_str).map(str::trim) {
            Some(existing) if !existing.is_empty() => {
                TaskUuid::parse(existing).with_context(|| {
                    format!("invalid task_uuid on {} row {}", kind.as_str(), index + 2)
                })?
            }
            _ => legacy_task_uuid(workspace_id, kind, display_id),
        };
        row.insert("task_uuid".to_owned(), task_uuid.to_string());
        if has_assigned_to {
            row.remove("assignee");
        } else {
            let assignment = row.remove("assignee").unwrap_or_default();
            row.insert("assigned_to".to_owned(), assignment);
        }
        row.entry("system_key".to_owned()).or_default();
        rows.push(row);
    }

    let mut writer = csv::WriterBuilder::new().from_writer(Vec::new());
    writer.write_record(&header)?;
    for row in rows {
        writer.write_record(
            header
                .iter()
                .map(|column| row.get(column).map_or("", String::as_str)),
        )?;
    }
    writer
        .into_inner()
        .map_err(csv::IntoInnerError::into_error)
        .map_err(Into::into)
}

fn reject_duplicate_columns(header: &[String], kind: CsvKind) -> Result<()> {
    let mut seen = HashSet::new();
    if let Some(column) = header.iter().find(|column| !seen.insert(column.as_str())) {
        bail!("{} CSV has duplicate column {column}", kind.as_str());
    }
    Ok(())
}

fn migrated_header(source: &[String]) -> Vec<String> {
    let has_assigned_to = source.iter().any(|column| column == "assigned_to");
    let mut header = vec!["task_uuid".to_owned()];
    for column in source {
        match column.as_str() {
            "task_uuid" => {}
            "assignee" if has_assigned_to => {}
            "assignee" => header.push("assigned_to".to_owned()),
            _ => header.push(column.clone()),
        }
    }
    if !header.iter().any(|column| column == "assigned_to") {
        header.push("assigned_to".to_owned());
    }
    if !header.iter().any(|column| column == "system_key") {
        header.push("system_key".to_owned());
    }
    header
}

fn migrate_schema_metadata(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut value: Value = serde_json::from_slice(bytes).context("parsing tasks/SCHEMA.json")?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| anyhow!("tasks/SCHEMA.json must contain a JSON object"))?;
    object.insert(
        "task_schema_version".to_owned(),
        Value::from(TASK_SCHEMA_VERSION),
    );
    object.insert(
        "merge_key".to_owned(),
        Value::String("task_uuid".to_owned()),
    );
    object.insert(
        "display_identity".to_owned(),
        Value::Object(Map::from_iter([
            ("field".to_owned(), Value::String("task_id".to_owned())),
            ("mutable".to_owned(), Value::Bool(true)),
        ])),
    );
    object.insert(
        "identity".to_owned(),
        json!({
            "task_uuid": "immutable UUID merge identity",
            "task_id": "mutable human-facing display identity"
        }),
    );
    let mut output = serde_json::to_vec_pretty(&value)?;
    output.push(b'\n');
    Ok(output)
}

fn stage(destination: &Path, bytes: &[u8]) -> Result<(PathBuf, PathBuf)> {
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("task schema destination has no UTF-8 filename"))?;
    let temporary = destination.with_file_name(format!(
        ".{file_name}.task-schema-{}.tmp",
        uuid::Uuid::new_v4()
    ));
    write_new(&temporary, bytes)?;
    Ok((temporary, destination.to_path_buf()))
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("creating {}", path.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("writing {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("syncing {}", path.display()))?;
    Ok(())
}

fn sync_parent(path: &Path) {
    if let Some(parent) = path.parent()
        && let Ok(directory) = fs::File::open(parent)
    {
        let _ = directory.sync_all();
    }
}
