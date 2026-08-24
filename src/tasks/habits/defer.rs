//! `brain habits defer <id>` — skip the next occurrence of a habit.
//!
//! Advances the due date by one recurrence interval (or N), using the same
//! anchor-to-due catch-up maths completion's spawn step uses, so a Monday
//! weekly habit stays on Mondays. No `completed_date` is recorded: the skipped
//! occurrence simply was not done.
//!
//! Habits have no `defer_count` — the recurrence *is* the deferral mechanism —
//! so nothing is incremented and nothing is demoted.

use std::path::Path;

use anyhow::{Result, bail};
use chrono::NaiveDate;

use crate::tasks::agenda::{Action, Outcome, Targets, sync_targets};
use crate::tasks::complete::{field, next_due, touch_row, write_csv};
use crate::tasks::mutate::locate_target;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeferHabitResult {
    pub(crate) task_id: String,
    pub(crate) task_name: String,
    pub(crate) old_due: String,
    pub(crate) new_due: String,
    pub(crate) occurrences: u32,
    pub(crate) interval: u32,
    pub(crate) unit: String,
}

pub(crate) fn defer_in_root(
    root: &Path,
    targets: &Targets,
    raw_id: &str,
    occurrences: u32,
    today: NaiveDate,
) -> Result<(DeferHabitResult, Outcome)> {
    let mut target = locate_target(root, raw_id)?;
    if !target.is_habit {
        let id = field(target.row()?, "task_id");
        bail!(
            "{id} is a task, not a habit; use `{}` instead",
            crate::workspace::suggest(&format!("tasks defer {id} +1d"))
        );
    }
    let occurrences = occurrences.max(1);
    let row = target.row()?;
    let interval = field(row, "recur_interval")
        .trim()
        .parse::<u32>()
        .unwrap_or(1);
    let unit = field(row, "recur_unit");
    let old_due = field(row, "due_date");

    let mut new_due = old_due.clone();
    for _ in 0..occurrences {
        new_due = next_due(&new_due, interval, &unit, today)?;
    }

    target.ensure_column("last_touched");
    let today_string = today.to_string();
    let row = target.row_mut()?;
    row.insert("due_date".to_owned(), new_due.clone());
    touch_row(row, &today_string);
    let result = DeferHabitResult {
        task_id: field(row, "task_id"),
        task_name: field(row, "task_name"),
        old_due,
        new_due,
        occurrences,
        interval,
        unit,
    };
    write_csv(&target.path, &target.csv)?;

    let outcome = sync_targets(targets, &result.task_id, Action::Defer, today);
    Ok((result, outcome))
}
