//! `brain tasks backlog <id>` — park a task, or bring one back.
//!
//! The backlog is for tasks parked indefinitely: not abandoned (that is
//! `remove`), just not on the active list. A parked task has **no schedule** —
//! due date, start date, and the waiting stamp are cleared, and a hard deadline
//! is meaningless without a due date — is hidden from every active view and
//! from the at-risk and chronic-ignore scans, resurfaces only in the monthly
//! backlog review, and is purged once it has sat for six months.
//!
//! Restoring flips it back to `not_started` and clears the parking stamp. It
//! deliberately does **not** invent a due date; the caller re-schedules it.

use std::path::Path;

use anyhow::{Result, bail};
use chrono::NaiveDate;

use super::locate_target;
use crate::tasks::agenda::{Action, Outcome, Targets, sync_targets};
use crate::tasks::complete::{field, touch_row, write_csv};

/// Columns a parked task has cleared.
const CLEARED: [&str; 3] = ["due_date", "start_date", "waiting_since"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BacklogResult {
    pub(crate) task_id: String,
    pub(crate) task_name: String,
    pub(crate) previous_status: String,
    pub(crate) restored: bool,
    /// True when the row was already where the caller asked for it.
    pub(crate) already: bool,
    /// The project this task belongs to, when it has one — whether the project
    /// should follow it into the backlog is the caller's judgement call.
    pub(crate) project: Option<String>,
}

pub(crate) fn backlog_in_root(
    root: &Path,
    targets: &Targets,
    raw_id: &str,
    restore: bool,
    today: NaiveDate,
) -> Result<(BacklogResult, Outcome)> {
    let mut target = locate_target(root, raw_id)?;
    if target.is_habit {
        let id = field(target.row()?, "task_id");
        bail!(
            "{id} is a habit and habits cannot be parked — a habit's recurrence already \
             decides when it comes back. Use `{}` or `{}`.",
            crate::workspace::suggest(&format!("habits defer {id}")),
            crate::workspace::suggest(&format!("habits skip {id}"))
        );
    }
    let today_string = today.to_string();
    let row = target.row()?;
    let task_id = field(row, "task_id");
    let task_name = field(row, "task_name");
    let previous_status = field(row, "status");
    let project = Some(field(row, "project"))
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let parked = previous_status.trim() == "backlog";

    if restore && !parked {
        bail!("{task_id} is not in the backlog (status={previous_status})");
    }
    let already = !restore && parked;

    if !already {
        target.ensure_column("backlogged_date");
        target.ensure_column("last_touched");
        let row = target.row_mut()?;
        if restore {
            row.insert("status".to_owned(), "not_started".to_owned());
            row.insert("backlogged_date".to_owned(), String::new());
        } else {
            row.insert("status".to_owned(), "backlog".to_owned());
            row.insert("backlogged_date".to_owned(), today_string.clone());
            row.insert("hard_deadline".to_owned(), "false".to_owned());
            for column in CLEARED {
                row.insert(column.to_owned(), String::new());
            }
        }
        touch_row(row, &today_string);
        write_csv(&target.path, &target.csv)?;
    }

    // Parking drops it from today's plan; restoring only makes the snapshots
    // stale, because nobody but the agenda's author decides today's order.
    let action = if restore {
        Action::Touch
    } else {
        Action::Defer
    };
    let outcome = if already {
        Outcome::Unchanged
    } else {
        sync_targets(targets, &task_id, action, today)
    };

    Ok((
        BacklogResult {
            task_id,
            task_name,
            previous_status,
            restored: restore,
            already,
            project,
        },
        outcome,
    ))
}
