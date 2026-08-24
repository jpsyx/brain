use std::path::Path;

use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use serde::Serialize;

use crate::actor::ActorContext;
use crate::tasks::complete::{CsvFile, Row, read_csv, write_csv};
use crate::tasks::identity::TaskUuid;

const TASK_COLUMNS: &[&str] = &[
    "task_uuid",
    "task_id",
    "task_name",
    "task_type",
    "status",
    "waiting_since",
    "priority",
    "due_date",
    "hard_deadline",
    "start_date",
    "assigned_to",
    "see_also",
    "notes",
    "project",
    "energy_level",
    "context",
    "estimated_duration",
    "blocked_by",
    "defer_count",
    "created_date",
    "completed_date",
    "last_touched",
    "linear_issue",
    "system_key",
];
const HABIT_COLUMNS: &[&str] = &[
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
    "recur_interval",
    "recur_unit",
    "ideal_time",
    "created_date",
    "completed_date",
    "last_touched",
    "system_key",
];

#[derive(Debug, Clone, Default)]
pub struct CreateRequest {
    pub name: String,
    pub task_type: Option<String>,
    pub priority: String,
    pub due: Option<String>,
    pub start: Option<String>,
    pub hard_deadline: bool,
    pub see_also: Option<String>,
    pub notes: Option<String>,
    pub project: Option<String>,
    pub energy: Option<String>,
    pub context: Option<String>,
    pub duration: Option<String>,
    pub blocked_by: Option<String>,
    pub assigned_to: Option<String>,
    pub linear_issue: Option<String>,
    pub habit: bool,
    pub interval: Option<u32>,
    pub unit: Option<String>,
    /// Time of day a habit is meant to happen (`6:45 AM`). Habits only — it is
    /// what the habits view groups Morning/Afternoon/Evening by.
    pub ideal_time: Option<String>,
    pub chunks: Option<u32>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CreatedRow {
    pub id: String,
    pub name: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CreateResult {
    pub created: Vec<CreatedRow>,
}

impl CreateResult {
    #[must_use]
    pub fn ids(&self) -> impl IntoIterator<Item = &str> {
        self.created.iter().map(|row| row.id.as_str())
    }
}

pub fn create_in_workspace(
    store: &crate::workspace::RegistryStore,
    workspace: &crate::workspace::WorkspaceContext,
    actor: &ActorContext,
    request: &CreateRequest,
) -> Result<CreateResult> {
    let _owner = crate::tasks::store_lock::TaskStoreOwner::acquire(workspace)?;
    let today = chrono::Local::now().date_naive();
    let targets = crate::tasks::agenda::resolve_targets(store, workspace, today);
    Ok(create_in_root_and_sync(workspace.root(), &targets, actor, request, today)?.0)
}

/// Create, then re-sync the agenda. A new row can land on today (a habit due
/// today joins Today's habits), so the CSV-derived snapshots are refreshed;
/// the authored plan is nobody's to write but the agenda's author.
pub(crate) fn create_in_root_and_sync(
    root: &Path,
    targets: &crate::tasks::agenda::Targets,
    actor: &ActorContext,
    request: &CreateRequest,
    today: NaiveDate,
) -> Result<(CreateResult, crate::tasks::agenda::Outcome)> {
    let result = create_in_root_for_actor_with_today(root, actor, request, today)?;
    let outcome =
        crate::tasks::agenda::sync_targets(targets, "", crate::tasks::agenda::Action::Touch, today);
    Ok((result, outcome))
}

pub fn create_in_root_for_actor_with_today(
    root: &Path,
    actor: &ActorContext,
    request: &CreateRequest,
    today: NaiveDate,
) -> Result<CreateResult> {
    validate(request)?;
    let tasks_dir = root.join("tasks");
    std::fs::create_dir_all(&tasks_dir)
        .with_context(|| format!("create task directory {}", tasks_dir.display()))?;
    let assigned_to = assignment(root, actor, request.assigned_to.as_deref())?;
    let common = common_row(request, &assigned_to, today);

    if request.habit {
        let path = tasks_dir.join("habits.csv");
        let mut csv = read_csv(&path)?;
        let id = next_id(&tasks_dir, &csv, 'H')?;
        let mut row = common;
        row.insert("task_id".to_owned(), id.clone());
        row.insert(
            "recur_interval".to_owned(),
            request.interval.unwrap().to_string(),
        );
        row.insert("recur_unit".to_owned(), request.unit.clone().unwrap());
        row.insert(
            "ideal_time".to_owned(),
            request.ideal_time.clone().unwrap_or_default(),
        );
        append(&mut csv, HABIT_COLUMNS, row.clone());
        write_csv(&path, &csv)?;
        return Ok(CreateResult {
            created: vec![CreatedRow {
                id,
                name: request.name.clone(),
                kind: "habit".to_owned(),
            }],
        });
    }

    let path = tasks_dir.join("tasks.csv");
    let mut csv = read_csv(&path)?;
    let count = request.chunks.unwrap_or(0);
    let mut created = Vec::new();
    let mut previous_id = String::new();
    for index in 1..=count.max(1) {
        let id = next_id(&tasks_dir, &csv, 'T')?;
        let mut row = common.clone();
        let name = if count > 0 {
            format!("{} ({index}/{count})", request.name)
        } else {
            request.name.clone()
        };
        if count > 0 {
            row.insert("task_uuid".to_owned(), TaskUuid::new().to_string());
        }
        row.insert("task_id".to_owned(), id.clone());
        row.insert("task_name".to_owned(), name.clone());
        row.insert(
            "task_type".to_owned(),
            if index == 1 {
                request.task_type.clone().unwrap()
            } else {
                strip_mit(request.task_type.as_deref().unwrap())
            },
        );
        row.insert("start_date".to_owned(), value(request.start.as_deref()));
        row.insert(
            "blocked_by".to_owned(),
            if index == 1 {
                value(request.blocked_by.as_deref())
            } else {
                previous_id.clone()
            },
        );
        row.insert("defer_count".to_owned(), "0".to_owned());
        row.insert(
            "linear_issue".to_owned(),
            if index == 1 {
                value(request.linear_issue.as_deref())
            } else {
                String::new()
            },
        );
        append(&mut csv, TASK_COLUMNS, row);
        created.push(CreatedRow {
            id: id.clone(),
            name,
            kind: "task".to_owned(),
        });
        previous_id = id;
        if count == 0 {
            break;
        }
    }
    write_csv(&path, &csv)?;
    Ok(CreateResult { created })
}

fn validate(request: &CreateRequest) -> Result<()> {
    if request.name.is_empty() {
        bail!("--name is required");
    }
    if !matches!(request.priority.as_str(), "p0" | "p1" | "p2" | "p3" | "p4") {
        bail!(
            "invalid value '{}' for --priority; expected p0, p1, p2, p3, or p4",
            request.priority
        );
    }
    if request.habit {
        if request.chunks.unwrap_or(0) != 0 {
            bail!("--chunks is not supported with --habit (habits recur, they don't chunk)");
        }
        if request.interval.is_none() || request.unit.is_none() {
            bail!("--habit requires --interval and --unit");
        }
        if !matches!(request.unit.as_deref(), Some("days" | "weeks" | "months")) {
            bail!("invalid value for --unit; expected days, weeks, or months");
        }
    } else {
        if request.task_type.as_deref().is_none_or(str::is_empty) {
            bail!("--type is required for non-habit tasks");
        }
        if request.ideal_time.is_some() {
            bail!("--ideal-time is only supported with --habit (tasks have no time-of-day slot)");
        }
        if let Some(chunks) = request.chunks {
            if chunks == 1 {
                bail!("--chunks must be >= 2 (a single 'chunk' is just a normal task)");
            }
            if chunks >= 2
                && request
                    .duration
                    .as_deref()
                    .unwrap_or_default()
                    .trim()
                    .is_empty()
            {
                bail!("--chunks requires --duration (per-chunk minutes)");
            }
        }
    }
    if let Some(value) = request.energy.as_deref()
        && !matches!(value, "high" | "medium" | "low")
    {
        bail!("invalid value for --energy");
    }
    if let Some(value) = request.context.as_deref()
        && !matches!(value, "home" | "office" | "computer" | "calls" | "errand")
    {
        bail!("invalid value for --context");
    }
    Ok(())
}

fn common_row(request: &CreateRequest, assigned_to: &str, today: NaiveDate) -> Row {
    let today = today.to_string();
    let mut row = Row::new();
    row.insert("task_uuid".to_owned(), TaskUuid::new().to_string());
    row.insert("task_name".to_owned(), request.name.clone());
    row.insert("status".to_owned(), "not_started".to_owned());
    row.insert("priority".to_owned(), request.priority.clone());
    row.insert("due_date".to_owned(), value(request.due.as_deref()));
    row.insert(
        "hard_deadline".to_owned(),
        request.hard_deadline.to_string(),
    );
    row.insert("assigned_to".to_owned(), assigned_to.to_owned());
    for (key, source) in [
        ("see_also", &request.see_also),
        ("notes", &request.notes),
        ("project", &request.project),
        ("energy_level", &request.energy),
        ("context", &request.context),
        ("estimated_duration", &request.duration),
    ] {
        row.insert(key.to_owned(), source.clone().unwrap_or_default());
    }
    row.insert("created_date".to_owned(), today.clone());
    row.insert("completed_date".to_owned(), String::new());
    row.insert("last_touched".to_owned(), today);
    row.insert("system_key".to_owned(), String::new());
    row
}

fn append(csv: &mut CsvFile, columns: &[&str], row: Row) {
    if csv.header.is_empty() {
        csv.header = columns.iter().map(|column| (*column).to_owned()).collect();
    }
    for column in columns {
        if !csv.header.iter().any(|existing| existing == column) {
            csv.header.push((*column).to_owned());
        }
    }
    csv.rows.push(row);
}

fn value(value: Option<&str>) -> String {
    value.unwrap_or_default().to_owned()
}

fn strip_mit(task_type: &str) -> String {
    task_type
        .split('|')
        .filter(|part| !part.is_empty() && *part != "mit")
        .collect::<Vec<_>>()
        .join("|")
}

fn assignment(root: &Path, actor: &ActorContext, explicit: Option<&str>) -> Result<String> {
    let Some(explicit) = explicit else {
        return Ok(actor.user_id().to_string());
    };
    let id = crate::users::UserId::parse(explicit).map_err(anyhow::Error::msg)?;
    let users = crate::users::UsersStore::load_from(&root.join(".config/users.json"))
        .with_context(|| {
            format!(
                "cannot validate assigned_to without {}/.config/users.json",
                root.display()
            )
        })?;
    if users.user(&id).is_none() {
        bail!("assigned_to '{explicit}' is not a workspace member");
    }
    Ok(explicit.to_owned())
}

fn next_id(tasks_dir: &Path, csv: &CsvFile, prefix: char) -> Result<String> {
    let filename = if prefix == 'H' {
        ".habits_next_id"
    } else {
        ".tasks_next_id"
    };
    let path = tasks_dir.join(filename);
    let next = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| text.trim().parse::<u32>().ok())
        .unwrap_or_else(|| {
            csv.rows
                .iter()
                .filter_map(|row| row.get("task_id"))
                .filter_map(|id| id.strip_prefix(prefix))
                .filter_map(|id| id.parse::<u32>().ok())
                .max()
                .unwrap_or(0)
                + 1
        });
    std::fs::write(&path, format!("{}\n", next + 1))?;
    Ok(format!("{prefix}{next}"))
}

#[cfg(test)]
mod tests;
