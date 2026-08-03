//! Reconcile portable config with the managed triage habit rows.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::Local;
use serde_json::Value;

use super::ManagedTriageHabit;
use super::purge::{ManagedIdentities, derived_changes, purge_rows};
use super::transaction::{FileChange, recover_pending, replace_group};
use crate::tasks::complete::{CsvFile, Row, field, serialize_csv};
use crate::tasks::identity::TaskUuid;
use crate::tasks::store_lock::TaskStoreOwner;
use crate::workspace::WorkspaceContext;

const HABIT_COLUMNS: [&str; 21] = [
    "task_uuid",
    "task_id",
    "task_name",
    "status",
    "priority",
    "due_date",
    "hard_deadline",
    "assigned_to",
    "see_also",
    "notes",
    "project",
    "energy_level",
    "context",
    "estimated_duration",
    "ideal_time",
    "recur_interval",
    "recur_unit",
    "created_date",
    "completed_date",
    "last_touched",
    "system_key",
];

pub fn apply_triage_habits_config(workspace: &WorkspaceContext, enabled: bool) -> Result<()> {
    let owner = TaskStoreOwner::acquire(workspace)?;
    apply_triage_habits_config_owned(workspace, enabled, &owner)
}

pub(crate) fn apply_triage_habits_config_owned(
    workspace: &WorkspaceContext,
    enabled: bool,
    owner: &TaskStoreOwner,
) -> Result<()> {
    let config_path = crate::settings::config_dir(workspace).join("config.json");
    validate_existing_config(&config_path)?;
    recover_pending(workspace, owner)?;
    let tasks_dir = workspace.root().join("tasks");
    let tasks_path = tasks_dir.join("tasks.csv");
    let habits_path = tasks_dir.join("habits.csv");
    let counter_path = tasks_dir.join(".habits_next_id");

    let tasks_before = read_optional(&tasks_path)?;
    let habits_before = read_optional(&habits_path)?;
    let config_before = read_optional(&config_path)?;
    let counter_before = read_optional(&counter_path)?;
    let mut tasks = parse_or_empty(tasks_before.as_deref())?;
    let mut habits = parse_or_empty(habits_before.as_deref())?;
    let mut changes = Vec::new();

    if enabled {
        let next = reconcile_enabled(
            &mut habits,
            workspace.local_user_id(),
            counter_before.as_deref(),
        );
        push_csv_change(&mut changes, habits_path, habits_before, &habits)?;
        let counter_after = format!("{next}\n").into_bytes();
        if counter_before.as_ref() != Some(&counter_after) {
            changes.push(FileChange {
                path: counter_path,
                before: counter_before,
                after: counter_after,
            });
        }
    } else {
        let identities = ManagedIdentities::collect_all(&[&tasks, &habits]);
        purge_rows(&mut tasks);
        purge_rows(&mut habits);
        push_csv_change(&mut changes, tasks_path, tasks_before, &tasks)?;
        push_csv_change(&mut changes, habits_path, habits_before, &habits)?;
        changes.extend(derived_changes(workspace.root(), &identities)?);
    }

    let mut config = config_before
        .as_deref()
        .map(|bytes| config_map_from_bytes(&config_path, bytes))
        .transpose()?
        .unwrap_or_default();
    config.insert("enable_triage_habits".to_owned(), Value::Bool(enabled));
    let mut config_after = serde_json::to_vec_pretty(&Value::Object(config))?;
    config_after.push(b'\n');
    if config_before.as_ref() != Some(&config_after) {
        changes.push(FileChange {
            path: config_path,
            before: config_before,
            after: config_after,
        });
    }

    replace_group(workspace, owner, &changes)
}

fn validate_existing_config(path: &Path) -> Result<()> {
    if let Some(bytes) = read_optional(path)? {
        config_map_from_bytes(path, &bytes)?;
    }
    Ok(())
}

fn config_map_from_bytes(path: &Path, bytes: &[u8]) -> Result<serde_json::Map<String, Value>> {
    let value: Value =
        serde_json::from_slice(bytes).with_context(|| format!("parsing {}", path.display()))?;
    value
        .as_object()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("{} must contain a JSON object", path.display()))
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
    }
}

fn parse_or_empty(bytes: Option<&[u8]>) -> Result<CsvFile> {
    bytes.map_or_else(
        || {
            Ok(CsvFile {
                header: Vec::new(),
                rows: Vec::new(),
            })
        },
        crate::tasks::complete::parse_csv_bytes,
    )
}

fn push_csv_change(
    changes: &mut Vec<FileChange>,
    path: std::path::PathBuf,
    before: Option<Vec<u8>>,
    csv: &CsvFile,
) -> Result<()> {
    let after = serialize_csv(csv)?;
    if before.as_ref() != Some(&after) {
        changes.push(FileChange {
            path,
            before,
            after,
        });
    }
    Ok(())
}

fn reconcile_enabled(habits: &mut CsvFile, assigned_to: &str, counter: Option<&[u8]>) -> u32 {
    ensure_columns(habits);
    let today = Local::now().date_naive().to_string();
    let floor = habits
        .rows
        .iter()
        .filter_map(|row| field(row, "task_id").strip_prefix('H')?.parse::<u32>().ok())
        .max()
        .unwrap_or(0)
        + 1;
    let mut next = counter
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .and_then(|value| value.trim().parse::<u32>().ok())
        .map_or(floor, |value| value.max(floor));
    for definition in ManagedTriageHabit::ALL {
        let mut has_open = false;
        habits.rows.retain(|row| {
            if field(row, "system_key") != definition.system_key || field(row, "status") == "done" {
                return true;
            }
            if has_open {
                return false;
            }
            has_open = true;
            true
        });
        if !has_open {
            habits
                .rows
                .push(new_row(definition, format!("H{next}"), assigned_to, &today));
            next += 1;
        }
    }
    next
}

fn ensure_columns(habits: &mut CsvFile) {
    for column in HABIT_COLUMNS {
        if !habits.header.iter().any(|existing| existing == column) {
            habits.header.push(column.to_owned());
        }
    }
}

fn new_row(definition: ManagedTriageHabit, task_id: String, assigned_to: &str, today: &str) -> Row {
    let mut row = BTreeMap::new();
    row.insert("task_uuid".to_owned(), TaskUuid::new().to_string());
    row.insert("task_id".to_owned(), task_id);
    row.insert("task_name".to_owned(), definition.name.to_owned());
    row.insert("status".to_owned(), "not_started".to_owned());
    row.insert("priority".to_owned(), "p1".to_owned());
    row.insert("due_date".to_owned(), today.to_owned());
    row.insert("hard_deadline".to_owned(), "false".to_owned());
    row.insert("assigned_to".to_owned(), assigned_to.to_owned());
    row.insert("recur_interval".to_owned(), definition.interval.to_owned());
    row.insert("recur_unit".to_owned(), definition.unit.to_owned());
    row.insert("created_date".to_owned(), today.to_owned());
    row.insert("last_touched".to_owned(), today.to_owned());
    row.insert("system_key".to_owned(), definition.system_key.to_owned());
    row
}
