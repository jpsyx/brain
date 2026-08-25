//! `brain tasks set <id> [--field value…]` — edit an existing task or habit.
//!
//! The absolute-value counterpart to `complete`: it rewrites named fields on
//! one row in place. This is the surface an external tracker mirrors onto (a
//! Linear issue's `dueDate`/`priority`/`title` change), which is exactly why it
//! does **not** touch `defer_count` — a due date moved by someone else is not
//! the user's slip. Relative, penalty-counting pushes stay with the defer path.
//!
//! A habit row requires the explicit `--habit` opt-in, mirroring
//! `remove_task.py`: rescheduling a habit is a legitimate act, but never
//! something a task-cleanup pass should reach by accident.
//!
//! [`plan`] is the pure decision (which columns change, to what, and every
//! rejection); the caller writes what it returns.

mod plan;

pub use plan::{Edit, FieldChange, SetPlan, plan};

use std::path::Path;

use anyhow::{Result, anyhow};
use chrono::{Local, NaiveDate};

use crate::tasks::complete::{Located, locate, read_csv, write_csv};

/// Apply an [`Edit`] to one row in the selected workspace, under the task-store
/// lock.
pub fn set_in_workspace(
    store: &crate::workspace::RegistryStore,
    workspace: &crate::workspace::WorkspaceContext,
    raw_id: &str,
    edit: &Edit,
) -> Result<SetPlan> {
    let _owner = crate::tasks::store_lock::TaskStoreOwner::acquire(workspace)?;
    let today = Local::now().date_naive();
    let targets = crate::tasks::agenda::resolve_targets(store, workspace, today);
    Ok(set_in_root_and_sync(workspace.root(), &targets, raw_id, edit, today)?.0)
}

/// Edit, then re-sync the agenda for whatever the edit means for today's plan.
pub(crate) fn set_in_root_and_sync(
    root: &Path,
    targets: &crate::tasks::agenda::Targets,
    raw_id: &str,
    edit: &Edit,
    today: NaiveDate,
) -> Result<(SetPlan, crate::tasks::agenda::Outcome)> {
    let planned = set_in_root_with_today(root, raw_id, edit, today)?;
    let outcome = crate::tasks::agenda::sync_targets(
        targets,
        &planned.task_id,
        agenda_action(&planned, today),
        today,
    );
    Ok((planned, outcome))
}

/// What an edit means for the day's plan.
///
/// An edit is not by itself a statement that the row left today: renaming it
/// or adding a note leaves it exactly where the agenda's author put it. Two
/// edits do say it left — being marked `done`, and being moved to another
/// day — and those are the two that drop it from the plan.
fn agenda_action(planned: &SetPlan, today: NaiveDate) -> crate::tasks::agenda::Action {
    let changed = |column: &str| {
        planned
            .changes
            .iter()
            .find(|change| change.column == column)
            .map(|change| change.after.trim())
    };
    if changed("status") == Some("done") {
        return crate::tasks::agenda::Action::Done;
    }
    match changed("due_date") {
        Some(after) if after != today.to_string() => crate::tasks::agenda::Action::Defer,
        _ => crate::tasks::agenda::Action::Touch,
    }
}

/// Root-scoped, clock-injected core. Reads both CSVs, resolves `raw_id`, plans
/// the edit, and writes only when something actually changed.
pub fn set_in_root_with_today(
    root: &Path,
    raw_id: &str,
    edit: &Edit,
    today: NaiveDate,
) -> Result<SetPlan> {
    let tasks_dir = root.join("tasks");
    let tasks_path = tasks_dir.join("tasks.csv");
    let habits_path = tasks_dir.join("habits.csv");
    let mut tasks = read_csv(&tasks_path)?;
    let mut habits = read_csv(&habits_path)?;
    let located = locate(&tasks, &habits, raw_id)?;
    let (path, csv, index, is_habit) = match located {
        Located::Task(index) => (&tasks_path, &mut tasks, index, false),
        Located::Habit(index) => (&habits_path, &mut habits, index, true),
    };
    let row = csv
        .rows
        .get(index)
        .ok_or_else(|| anyhow!("task row disappeared"))?;
    let planned = plan(row, edit, is_habit, today)?;
    if planned.changes.is_empty() {
        return Ok(planned);
    }
    let row = csv
        .rows
        .get_mut(index)
        .ok_or_else(|| anyhow!("task row disappeared"))?;
    for change in &planned.changes {
        row.insert(change.column.clone(), change.after.clone());
    }
    row.insert("last_touched".to_owned(), today.to_string());
    for column in planned.columns_to_ensure() {
        if !csv.header.iter().any(|existing| existing == &column) {
            csv.header.push(column);
        }
    }
    write_csv(path, csv)?;
    Ok(planned)
}

#[cfg(test)]
mod tests;
