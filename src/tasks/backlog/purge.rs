//! Purging tasks parked for more than six months.
//!
//! The premise: something parked and untouched for half a year has been
//! forgotten, and is fine to forget forever. So the purge is silent — no
//! warning, no announcement.
//!
//! The one thing it is careful about is **project bookkeeping**. If a purged
//! task belonged to a project, active or archived, a breadcrumb is left in that
//! project so a future un-archive knows tasks used to exist: an entry in
//! `.METADATA.json`'s `deleted_backlog_tasks`, the id dropped from its live
//! `tasks` array, and a line under a "Deleted backlog tasks" heading in
//! `notes.md`.

use chrono::NaiveDate;
use serde::Serialize;

use crate::tasks::complete::{Row, field, parse_date};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct PurgedTask {
    pub(crate) task_id: String,
    pub(crate) task_name: String,
    pub(crate) backlogged_date: String,
    pub(crate) project: String,
}

/// Pure: which rows are past the cutoff.
pub(crate) fn expired(rows: &[Row], cutoff: NaiveDate) -> Vec<PurgedTask> {
    rows.iter()
        .filter(|row| field(row, "status").trim() == "backlog")
        .filter(|row| {
            parse_date(&field(row, "backlogged_date")).is_some_and(|parked| parked < cutoff)
        })
        .map(|row| PurgedTask {
            task_id: field(row, "task_id"),
            task_name: field(row, "task_name"),
            backlogged_date: field(row, "backlogged_date"),
            project: field(row, "project").trim().to_owned(),
        })
        .collect()
}

/// The line appended to a project's `notes.md`.
pub(crate) fn breadcrumb_line(task: &PurgedTask, deleted: NaiveDate) -> String {
    format!(
        "- **{}** {}: backlogged {}, auto-deleted {} (>6mo in backlog). \
         Restore from git history if needed.\n",
        task.task_id, task.task_name, task.backlogged_date, deleted
    )
}

const NOTES_HEADING: &str = "## Deleted backlog tasks\n";

/// Pure composition from one original notes before-image.
pub(crate) fn append_breadcrumbs(
    original: &str,
    tasks: &[PurgedTask],
    deleted: NaiveDate,
) -> String {
    let mut result = original.to_owned();
    if !result.is_empty() && !result.ends_with('\n') {
        result.push('\n');
    }
    if !result.contains(NOTES_HEADING) {
        result.push('\n');
        result.push_str(NOTES_HEADING);
    }
    for task in tasks {
        result.push_str(&breadcrumb_line(task, deleted));
    }
    result
}
pub(crate) fn history_entry(task: &PurgedTask, deleted: NaiveDate) -> serde_json::Value {
    serde_json::json!({"task_id": task.task_id, "task_name": task.task_name, "backlogged_date": task.backlogged_date, "deleted_date": deleted.to_string()})
}
