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
    workspace: &crate::workspace::WorkspaceContext,
    raw_id: &str,
    edit: &Edit,
) -> Result<SetPlan> {
    let _owner = crate::tasks::store_lock::TaskStoreOwner::acquire(workspace)?;
    set_in_root_with_today(workspace.root(), raw_id, edit, Local::now().date_naive())
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
