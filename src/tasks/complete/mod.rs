//! `tasks complete <id>` — mark a task done in `~/brain/tasks/{tasks,habits}.csv`.
//!
//! Native Rust completion: set status/completed_date/last_touched, spawn the
//! next habit occurrence, and migrate chunked-task `mit` to the next chunk.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Result, anyhow, bail};
use chrono::{Datelike, Local, NaiveDate};

use crate::tasks::identity::TaskUuid;
use crate::theme::Theme;

pub(crate) type Row = BTreeMap<String, String>;

pub(crate) fn field(row: &Row, column: &str) -> String {
    row.get(column).cloned().unwrap_or_default()
}

fn name(row: &Row) -> String {
    nonempty(row, "task_name").unwrap_or_else(|| "(unnamed)".to_owned())
}

fn nonempty(row: &Row, column: &str) -> Option<String> {
    let value = field(row, column);
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CsvFile {
    pub(crate) header: Vec<String>,
    pub(crate) rows: Vec<Row>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Task,
    Habit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionResult {
    pub kind: CompletionKind,
    pub task_id: String,
    pub task_name: String,
    pub next_id: Option<String>,
    pub next_due: Option<String>,
    pub mit_migrated_to: Option<String>,
    pub project: Option<String>,
    pub linear_issue: Option<String>,
}

/// Normalize a user-supplied ID into the canonical `T###` / `H###` form.
///
/// Accepts: `t123`, `T123`, `123` (assumed task), `h43`, `H43`. Any other
/// shape returns an error explaining the accepted forms.
pub fn normalize_id(raw: &str) -> Result<String> {
    let s = raw.trim();
    if s.is_empty() {
        bail!("ID is required (try t123, T123, 123, or h43)");
    }
    let lower = s.to_ascii_lowercase();
    let (prefix, digits) = match lower.as_bytes().first() {
        Some(b't') => ('T', &lower[1..]),
        Some(b'h') => ('H', &lower[1..]),
        _ => ('T', lower.as_str()),
    };

    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        bail!("'{raw}' is not a valid ID (try t123, T123, 123, or h43)");
    }
    // Parse + reformat to drop any leading zeros (T0123 → T123) but keep the
    // exact value the user meant.
    let n: u32 = digits
        .parse()
        .map_err(|e| anyhow!("invalid number in ID '{raw}': {e}"))?;
    Ok(format!("{prefix}{n}"))
}

pub fn run(
    workspace: &crate::workspace::WorkspaceContext,
    raw_id: &str,
    actor: &crate::actor::ActorContext,
) -> Result<()> {
    let root = workspace.root();
    crate::logging::log(format!("tasks complete raw_id={raw_id}"));
    crate::logging::log(format!("complete root {}", root.display()));
    let today = Local::now().date_naive();
    let result = complete_in_workspace_for_actor_with_today(workspace, raw_id, today, actor)?;
    crate::logging::log(format!(
        "complete result kind={:?} id={}",
        result.kind, result.task_id
    ));
    print_result(&result);
    Ok(())
}

pub(crate) fn complete_in_workspace_for_actor_with_today(
    workspace: &crate::workspace::WorkspaceContext,
    raw_id: &str,
    today: NaiveDate,
    actor: &crate::actor::ActorContext,
) -> Result<CompletionResult> {
    let _owner = crate::tasks::store_lock::TaskStoreOwner::acquire(workspace)?;
    complete_in_root_for_actor_with_today(workspace.root(), raw_id, today, actor)
}

/// Complete one task on behalf of a caller that already owns the task store.
pub(crate) fn complete_in_root_with_owner_and_today(
    root: &Path,
    lock_path: &Path,
    owner: &crate::tasks::store_lock::TaskStoreOwner,
    raw_id: &str,
    today: NaiveDate,
) -> Result<CompletionResult> {
    owner.verify_path(lock_path)?;
    complete_in_root_with_today(root, raw_id, today)
}

/// Complete one task under the actor bound at the request boundary.
pub(crate) fn complete_in_root_for_actor_with_today(
    root: &Path,
    raw_id: &str,
    today: NaiveDate,
    _actor: &crate::actor::ActorContext,
) -> Result<CompletionResult> {
    complete_in_root_with_today(root, raw_id, today)
}

pub(crate) fn complete_in_root_with_today(
    root: &Path,
    raw_id: &str,
    today: NaiveDate,
) -> Result<CompletionResult> {
    let tasks_dir = root.join("tasks");
    let tasks_path = tasks_dir.join("tasks.csv");
    let habits_path = tasks_dir.join("habits.csv");
    if let Ok(normalized) = normalize_id(raw_id) {
        crate::logging::log(format!("complete normalized_id={normalized}"));
    }
    crate::logging::log(format!("read tasks csv {}", tasks_path.display()));
    let mut tasks = read_csv(&tasks_path)?;
    crate::logging::log(format!("read habits csv {}", habits_path.display()));
    let mut habits = read_csv(&habits_path)?;
    let located = locate(&tasks, &habits, raw_id)?;
    match located {
        Located::Task(idx) => {
            let result = complete_task(&mut tasks, idx, today)?;
            crate::logging::log(format!("write tasks csv {}", tasks_path.display()));
            write_csv(&tasks_path, &tasks)?;
            Ok(result)
        }
        Located::Habit(idx) => {
            let result = complete_habit(&tasks_dir, &mut habits, idx, today)?;
            crate::logging::log(format!("write habits csv {}", habits_path.display()));
            write_csv(&habits_path, &habits)?;
            Ok(result)
        }
    }
}

fn print_result(result: &CompletionResult) {
    let theme = Theme::active();
    match result.kind {
        CompletionKind::Task => {
            eprintln!(
                "{} {}  {}",
                theme.success("done:"),
                theme.accent(&result.task_id),
                theme.value(&result.task_name)
            );
            if let Some(id) = &result.mit_migrated_to {
                eprintln!("  {} {}", theme.info("MIT migrated to"), theme.accent(id));
            }
            if let Some(project) = &result.project {
                eprintln!(
                    "  {} {}; {}",
                    theme.warning("still linked to project"),
                    theme.value(project),
                    theme.muted("run /todo reindex to refresh")
                );
            }
            if let Some(issue) = &result.linear_issue {
                eprintln!(
                    "  {} {} {}",
                    theme.warning("LINEAR:"),
                    theme.accent(issue),
                    theme.muted("close this issue too")
                );
            }
        }
        CompletionKind::Habit => {
            eprintln!(
                "{} {}  {}  {}",
                theme.success("done:"),
                theme.accent(&result.task_id),
                theme.value(&result.task_name),
                theme.muted("(habit)")
            );
            if let (Some(id), Some(due)) = (&result.next_id, &result.next_due) {
                eprintln!(
                    "  {} {} {} {}",
                    theme.info("next occurrence:"),
                    theme.accent(id),
                    theme.muted("due"),
                    theme.value(due)
                );
            }
        }
    }
}

mod complete_ops;
pub(crate) use complete_ops::*;
#[cfg(test)]
mod tests;
