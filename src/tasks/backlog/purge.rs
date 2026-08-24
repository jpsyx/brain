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

use std::path::{Path, PathBuf};

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

/// Locate a project directory by slug, under `projects/` or anywhere in
/// `archive/`. An archived project still deserves its breadcrumb.
pub(crate) fn find_project_dir(root: &Path, slug: &str) -> Option<PathBuf> {
    if slug.is_empty() {
        return None;
    }
    let active = root.join("projects").join(slug);
    if active.join(".METADATA.json").is_file() {
        return Some(active);
    }
    let archive = root.join("archive");
    if !archive.is_dir() {
        return None;
    }
    walkdir::WalkDir::new(archive)
        .into_iter()
        .filter_map(Result::ok)
        .find(|entry| {
            entry.file_name() == ".METADATA.json"
                && entry
                    .path()
                    .parent()
                    .and_then(Path::file_name)
                    .is_some_and(|name| name == slug)
        })
        .and_then(|entry| entry.path().parent().map(Path::to_path_buf))
}

/// The line appended to a project's `notes.md`.
pub(crate) fn breadcrumb_line(task: &PurgedTask, deleted: NaiveDate) -> String {
    format!(
        "- **{}** {} — backlogged {}, auto-deleted {} (>6mo in backlog). \
         Restore from git history if needed.\n",
        task.task_id, task.task_name, task.backlogged_date, deleted
    )
}

const NOTES_HEADING: &str = "## Deleted backlog tasks\n";

/// Append the breadcrumb to `notes.md`, creating the heading once.
pub(crate) fn append_breadcrumb(project: &Path, task: &PurgedTask, deleted: NaiveDate) {
    let notes = project.join("notes.md");
    let mut existing = std::fs::read_to_string(&notes).unwrap_or_default();
    if !existing.contains(NOTES_HEADING) {
        if !existing.is_empty() && !existing.ends_with('\n') {
            existing.push('\n');
        }
        existing.push('\n');
        existing.push_str(NOTES_HEADING);
    }
    existing.push_str(&breadcrumb_line(task, deleted));
    let _ = std::fs::write(&notes, existing);
}

/// Record the deletion in the project's `.METADATA.json`.
pub(crate) fn record_in_metadata(project: &Path, task: &PurgedTask, deleted: NaiveDate) {
    let path = project.join(".METADATA.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return;
    };
    let Some(object) = value.as_object_mut() else {
        return;
    };
    let entry = serde_json::json!({
        "task_id": task.task_id,
        "task_name": task.task_name,
        "backlogged_date": task.backlogged_date,
        "deleted_date": deleted.to_string(),
    });
    if let Some(entries) = object
        .entry("deleted_backlog_tasks")
        .or_insert_with(|| serde_json::Value::Array(Vec::new()))
        .as_array_mut()
    {
        entries.push(entry);
    }
    if let Some(tasks) = object
        .get_mut("tasks")
        .and_then(serde_json::Value::as_array_mut)
    {
        tasks.retain(|id| id.as_str() != Some(task.task_id.as_str()));
    }
    if let Ok(rendered) = serde_json::to_string_pretty(&value) {
        let _ = std::fs::write(&path, rendered + "\n");
    }
}
