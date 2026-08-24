//! `brain tasks remove <id>` — delete one row, with the habit chain protected.
//!
//! `locate` searches both CSVs, so a needle meant for a task can land on a
//! habit. Deleting a habit row destroys the whole recurring chain, every future
//! occurrence with it, so it is refused without an explicit opt-in — which task
//! cleanup passes never pass, and therefore structurally cannot delete a habit.

use std::path::Path;

use anyhow::{Result, bail};
use chrono::NaiveDate;

use super::locate_target;
use crate::tasks::agenda::{Action, Outcome, Targets, sync_targets};
use crate::tasks::complete::{field, write_csv};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemoveResult {
    pub(crate) task_id: String,
    pub(crate) task_name: String,
    pub(crate) was_habit: bool,
}

/// Remove `raw_id`, then drop it from the day's agenda.
pub(crate) fn remove_in_root(
    root: &Path,
    targets: &Targets,
    raw_id: &str,
    allow_habit: bool,
    today: NaiveDate,
) -> Result<(RemoveResult, Outcome)> {
    let mut target = locate_target(root, raw_id)?;
    let row = target.row()?;
    let task_id = field(row, "task_id");
    let task_name = field(row, "task_name");
    let label = format!("{task_id}  {task_name}");

    if target.is_habit {
        crate::tasks::triage_habits::protect_system_key(
            &field(row, "system_key"),
            crate::config::Config::load_from_root(root).enable_triage_habits,
            crate::tasks::triage_habits::ManagedTaskError::ManagedTaskCannotDelete,
        )?;
        if !allow_habit {
            bail!(
                "refusing to remove habit {label}: deleting a habit row destroys its whole \
                 recurring chain, including every future occurrence.\nHabits are never part of \
                 task cleanup. Pass --habit if you really mean to retire this habit, or use \
                 `{}` to push the next occurrence out instead.",
                crate::workspace::suggest(&format!("habits defer {task_id}"))
            );
        }
    } else if allow_habit {
        bail!("--habit was passed but {label} is a task, not a habit; re-run without --habit");
    }

    target.csv.rows.remove(target.index);
    write_csv(&target.path, &target.csv)?;

    let outcome = sync_targets(targets, &task_id, Action::Defer, today);
    Ok((
        RemoveResult {
            task_id,
            task_name,
            was_habit: target.is_habit,
        },
        outcome,
    ))
}
