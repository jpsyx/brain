//! The pure decision behind `brain tasks set`: validate the requested edit
//! against one row and return exactly which columns change.
//!
//! Everything that can be rejected is rejected here, before any write: an
//! unknown priority or status, an unparseable date, a habit touched without
//! `--habit`, a task given `--habit`, an empty edit, or a rename to blank.

use anyhow::{Result, bail};
use chrono::NaiveDate;

use crate::tasks::complete::{Row, field};

/// Valid priority values, lowest index = most urgent.
const PRIORITIES: [&str; 5] = ["p0", "p1", "p2", "p3", "p4"];
/// Valid task statuses.
const STATUSES: [&str; 5] = ["not_started", "in_progress", "waiting", "done", "backlog"];

/// One requested edit. Every field is optional; at least one must be set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Edit {
    pub name: Option<String>,
    pub due: Option<String>,
    pub priority: Option<String>,
    pub status: Option<String>,
    pub notes: Option<String>,
    pub project: Option<String>,
    pub linear_issue: Option<String>,
    pub duration: Option<String>,
    pub ideal_time: Option<String>,
    /// Explicit opt-in required to edit a habit row, and refused for a task.
    pub habit: bool,
}

impl Edit {
    /// True when no field was requested (the caller should prompt or error).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.due.is_none()
            && self.priority.is_none()
            && self.status.is_none()
            && self.notes.is_none()
            && self.project.is_none()
            && self.linear_issue.is_none()
            && self.duration.is_none()
            && self.ideal_time.is_none()
    }
}

/// One column's before/after, only present when the value actually changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldChange {
    pub column: String,
    pub before: String,
    pub after: String,
}

/// The planned outcome of one `set`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetPlan {
    pub task_id: String,
    pub task_name: String,
    pub is_habit: bool,
    pub changes: Vec<FieldChange>,
}

impl SetPlan {
    /// Columns that must exist in the header before writing.
    #[must_use]
    pub fn columns_to_ensure(&self) -> Vec<String> {
        let mut columns: Vec<String> = self
            .changes
            .iter()
            .map(|change| change.column.clone())
            .collect();
        columns.push("last_touched".to_owned());
        columns
    }

    /// True when the row already held every requested value.
    #[must_use]
    pub fn is_noop(&self) -> bool {
        self.changes.is_empty()
    }
}

/// Validate `edit` against `row` and return the columns that change.
pub fn plan(row: &Row, edit: &Edit, is_habit: bool, today: NaiveDate) -> Result<SetPlan> {
    let task_id = field(row, "task_id");
    if edit.is_empty() {
        bail!(
            "no fields to set for {task_id}; pass at least one of --name, --due, --priority, \
             --status, --notes, --project, --linear-issue, --duration, --ideal-time"
        );
    }
    if is_habit && !edit.habit {
        bail!(
            "{task_id} is a habit; pass --habit to edit it. Rescheduling a habit is never part \
             of task cleanup — prefer `brain habits skip` to move one occurrence"
        );
    }
    if edit.habit && !is_habit {
        bail!("--habit was passed but {task_id} is a task; re-run without --habit");
    }
    if edit.ideal_time.is_some() && !is_habit {
        bail!("--ideal-time is only supported for habits (tasks have no time-of-day slot)");
    }

    let mut changes = Vec::new();
    let mut set = |column: &str, requested: Option<&String>| -> Result<()> {
        let Some(requested) = requested else {
            return Ok(());
        };
        let before = field(row, column);
        if before == *requested {
            return Ok(());
        }
        changes.push(FieldChange {
            column: column.to_owned(),
            before,
            after: requested.clone(),
        });
        Ok(())
    };

    if let Some(name) = &edit.name {
        if name.trim().is_empty() {
            bail!("--name cannot be blank");
        }
        set("task_name", Some(name))?;
    }
    if let Some(due) = &edit.due {
        let normalized = normalize_due(due, today)?;
        set("due_date", Some(&normalized))?;
    }
    if let Some(priority) = &edit.priority {
        let normalized = priority.trim().to_ascii_lowercase();
        if !PRIORITIES.contains(&normalized.as_str()) {
            bail!("invalid value '{priority}' for --priority; expected p0, p1, p2, p3, or p4");
        }
        set("priority", Some(&normalized))?;
    }
    if let Some(status) = &edit.status {
        let normalized = status.trim().to_ascii_lowercase();
        if !STATUSES.contains(&normalized.as_str()) {
            bail!(
                "invalid value '{status}' for --status; expected one of {}",
                STATUSES.join(", ")
            );
        }
        set("status", Some(&normalized))?;
    }
    set("notes", edit.notes.as_ref())?;
    set("project", edit.project.as_ref())?;
    set("linear_issue", edit.linear_issue.as_ref())?;
    set("estimated_duration", edit.duration.as_ref())?;
    set("ideal_time", edit.ideal_time.as_ref())?;

    Ok(SetPlan {
        task_id,
        task_name: field(row, "task_name"),
        is_habit,
        changes,
    })
}

/// Accept `YYYY-MM-DD`, `today`, `tomorrow`, or an empty string (clear the
/// date). Rejects anything else rather than guessing.
fn normalize_due(raw: &str, today: NaiveDate) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    match trimmed.to_ascii_lowercase().as_str() {
        "today" => return Ok(today.to_string()),
        "tomorrow" => {
            return Ok((today + chrono::Days::new(1)).to_string());
        }
        _ => {}
    }
    match NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
        Ok(date) => Ok(date.to_string()),
        Err(_) => bail!("invalid --due '{raw}'; expected YYYY-MM-DD, today, tomorrow, or empty"),
    }
}
