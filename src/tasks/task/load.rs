//! CSV row structs (mirroring `tasks.csv` / `habits.csv`), date parsing, and
//! the loaders that deserialize a file into normalized [`Task`]s.

use std::path::Path;

use anyhow::{Context, Result};
use chrono::NaiveDate;
use serde::Deserialize;

use super::Task;

/// One-to-one row mapping for the on-disk CSV. Several fields aren't yet
/// surfaced in the UI but are kept so serde deserializes the full column set.
#[derive(Debug, Deserialize)]
pub struct TaskRow {
    pub task_id: String,
    pub task_name: String,
    pub task_type: String,
    pub status: String,
    pub priority: String,
    pub due_date: String,
    pub hard_deadline: String,
    pub start_date: String,
    #[allow(dead_code)]
    pub assignee: String,
    pub see_also: String,
    pub notes: String,
    pub project: String,
    pub energy_level: String,
    pub context: String,
    pub estimated_duration: String,
    pub blocked_by: String,
    pub defer_count: String,
    #[allow(dead_code)]
    pub created_date: String,
    pub completed_date: String,
    pub last_touched: String,
    /// Linear issue identifier (e.g. `AVA-123`) for code tasks mirrored to
    /// Linear; empty for unlinked / non-code tasks. Last column in tasks.csv.
    pub linear_issue: String,
}

/// One row of `habits.csv`. Mostly overlaps tasks but has its own recurrence
/// fields (`recur_interval`, `recur_unit`) and no `task_type` / `blocked_by`.
/// We map both into the shared `Task` struct so the rendering pipeline doesn't
/// need a separate path.
#[derive(Debug, Deserialize)]
pub struct HabitRow {
    pub task_id: String,
    pub task_name: String,
    pub status: String,
    pub priority: String,
    pub due_date: String,
    pub hard_deadline: String,
    #[allow(dead_code)]
    pub assignee: String,
    pub see_also: String,
    pub notes: String,
    pub project: String,
    pub energy_level: String,
    pub context: String,
    pub estimated_duration: String,
    #[allow(dead_code)]
    pub ideal_time: String,
    // recur_interval, recur_unit, completed_date, created_date are
    // deserialized so the column layout matches habits.csv, but not
    // currently consumed — the "due today?" check is purely
    // due_date + status driven.
    #[allow(dead_code)]
    pub recur_interval: String,
    #[allow(dead_code)]
    pub recur_unit: String,
    #[allow(dead_code)]
    pub created_date: String,
    pub completed_date: String,
    #[serde(default)]
    pub last_touched: String,
}

impl Task {
    fn from_row(row: TaskRow) -> Self {
        Self {
            id: row.task_id,
            name: row.task_name,
            types: Self::split_pipe(&row.task_type),
            status: row.status,
            priority: row.priority,
            due_date: parse_date_field(&row.due_date),
            hard_deadline: row.hard_deadline.eq_ignore_ascii_case("true"),
            start_date: parse_date_field(&row.start_date),
            notes: row.notes,
            project: row.project,
            energy: row.energy_level,
            context: row.context,
            estimated_duration: row.estimated_duration.parse().ok(),
            defer_count: row.defer_count.parse().unwrap_or(0),
            last_touched: parse_date_field(&row.last_touched),
            see_also: row.see_also,
            blocked_by: Self::split_pipe(&row.blocked_by),
            completed_date: parse_date_field(&row.completed_date),
            linear_issue: row.linear_issue,
        }
    }

    fn from_habit_row(row: HabitRow) -> Self {
        Self {
            id: row.task_id,
            name: row.task_name,
            types: Vec::new(),
            status: row.status,
            priority: row.priority,
            due_date: parse_date_field(&row.due_date),
            hard_deadline: row.hard_deadline.eq_ignore_ascii_case("true"),
            start_date: None,
            notes: row.notes,
            project: row.project,
            energy: row.energy_level,
            context: row.context,
            estimated_duration: row.estimated_duration.parse().ok(),
            defer_count: 0,
            last_touched: parse_date_field(&row.last_touched),
            see_also: row.see_also,
            blocked_by: Vec::new(),
            completed_date: parse_date_field(&row.completed_date),
            linear_issue: String::new(),
        }
    }
}

/// Accept either `YYYY-MM-DD` or `YYYY-MM-DDTHH:MM` (date portion only).
#[must_use]
pub fn parse_date_field(s: &str) -> Option<NaiveDate> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let date_part = s.split('T').next().unwrap_or(s);
    NaiveDate::parse_from_str(date_part, "%Y-%m-%d").ok()
}

pub fn load_tasks(path: &Path) -> Result<Vec<Task>> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(path)
        .with_context(|| format!("opening {}", path.display()))?;

    let mut out = Vec::new();
    for (i, result) in rdr.deserialize::<TaskRow>().enumerate() {
        let row = result.with_context(|| format!("parsing row {}", i + 2))?;
        out.push(Task::from_row(row));
    }
    Ok(out)
}

/// Load `habits.csv`. A missing file resolves to an empty list rather than
/// an error so the habits view degrades gracefully on setups without one.
pub fn load_habits(path: &Path) -> Result<Vec<Task>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(path)
        .with_context(|| format!("opening {}", path.display()))?;

    let mut out = Vec::new();
    for (i, result) in rdr.deserialize::<HabitRow>().enumerate() {
        let row = result.with_context(|| format!("parsing row {}", i + 2))?;
        out.push(Task::from_habit_row(row));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{load_habits, parse_date_field};
    use chrono::NaiveDate;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn parse_date_accepts_iso_date() {
        assert_eq!(parse_date_field("2026-06-23"), Some(d(2026, 6, 23)));
    }

    #[test]
    fn parse_date_accepts_iso_datetime_using_date_only() {
        assert_eq!(parse_date_field("2026-06-23T09:00"), Some(d(2026, 6, 23)));
    }

    #[test]
    fn parse_date_trims_whitespace() {
        assert_eq!(parse_date_field("  2026-06-23  "), Some(d(2026, 6, 23)));
    }

    #[test]
    fn parse_date_empty_returns_none() {
        assert!(parse_date_field("").is_none());
        assert!(parse_date_field("   ").is_none());
    }

    #[test]
    fn parse_date_malformed_returns_none() {
        assert!(parse_date_field("not-a-date").is_none());
        assert!(parse_date_field("2026/06/23").is_none());
    }

    #[test]
    fn load_habits_reads_last_touched_when_present() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("habits.csv");
        std::fs::write(
            &path,
            "task_id,task_name,status,priority,due_date,hard_deadline,assignee,see_also,notes,project,energy_level,context,estimated_duration,ideal_time,recur_interval,recur_unit,created_date,completed_date,last_touched\n\
H1,Stretch,not_started,p2,2026-07-27,false,me,,,,,,,,1,days,2026-07-01,,2026-07-26\n",
        )
        .unwrap();

        let habits = load_habits(&path).unwrap();

        assert_eq!(habits[0].last_touched, Some(d(2026, 7, 26)));
    }
}
