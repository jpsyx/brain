//! Task data model + CSV loading. Mirrors `~/brain/tasks/SCHEMA.json`.
//!
//! This module owns the normalized [`Task`] struct and its predicates;
//! [`load`] owns the on-disk CSV row structs and the loaders that turn a
//! file into `Vec<Task>`.

mod assignment;
mod load;

pub use assignment::{
    AssignmentContext, AssignmentUiMode, AssignmentUser, assignment_after_edit,
    assignment_context_for_workspace, assignment_filter_for_startup, assignment_for_create,
    assignment_ui_mode,
};
pub use load::{load_habits, load_tasks};

use chrono::NaiveDate;

use super::identity::TaskUuid;

/// Normalized, in-memory view of a task (or habit — same struct) with dates
/// parsed and pipe-lists split. Habit-only fields default to empty / zero
/// when loaded from tasks.csv.
#[derive(Debug, Clone)]
pub struct Task {
    /// Immutable merge identity. `None` is accepted only for legacy rows until rollout.
    pub task_uuid: Option<TaskUuid>,
    pub id: String,
    pub name: String,
    pub types: Vec<String>,
    pub status: String,
    pub priority: String,
    pub due_date: Option<NaiveDate>,
    pub hard_deadline: bool,
    pub start_date: Option<NaiveDate>,
    /// Portable workspace member currently responsible for this row.
    pub assigned_to: String,
    pub notes: String,
    pub project: String,
    pub energy: String,
    pub context: String,
    pub estimated_duration: Option<u32>,
    pub defer_count: u32,
    pub last_touched: Option<NaiveDate>,
    pub see_also: String,
    pub blocked_by: Vec<String>,
    pub completed_date: Option<NaiveDate>,
    /// Linear issue identifier (e.g. `AVA-123`) for tasks mirrored to
    /// Linear; empty for unlinked / non-code tasks (and always empty for
    /// habits, which never link to Linear).
    pub linear_issue: String,
    /// Stable key for a Brain-managed definition, empty for ordinary rows.
    pub system_key: String,
}

impl Task {
    pub(crate) fn split_pipe(s: &str) -> Vec<String> {
        s.split('|')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(String::from)
            .collect()
    }

    /// True iff the entry came from `habits.csv` — i.e. its canonical ID
    /// uses the `H` prefix.
    #[must_use]
    pub fn is_habit(&self) -> bool {
        self.id
            .chars()
            .next()
            .is_some_and(|c| c.eq_ignore_ascii_case(&'H'))
    }

    /// Whether this habit should appear in today's habits view. The rule
    /// is intentionally tight: show only habits that are due today or
    /// overdue AND not yet done. Future-dated habits (e.g. monthly
    /// recurrence that lands next week) are hidden until their cycle
    /// rolls around. Habits already completed for the current cycle are
    /// hidden too — the `/todo` machinery owns flipping `status` back to
    /// `not_started` and advancing `due_date` when the recurrence
    /// elapses.
    #[must_use]
    pub fn is_habit_due_today(&self, today: NaiveDate) -> bool {
        if self.is_done() {
            return false;
        }
        self.due_date.is_some_and(|d| d <= today)
    }

    #[must_use]
    pub fn is_done(&self) -> bool {
        self.status == "done"
    }

    /// Whether this row belongs to one of Brain's protected triage chains.
    #[must_use]
    pub fn is_managed_triage(&self) -> bool {
        crate::tasks::triage_habits::is_managed_system_key(&self.system_key)
    }

    /// True iff this row's `status` is `done` AND `completed_date` is
    /// `today`. Used by the startup-triage check to decide whether the
    /// configured daily-triage habit has already been completed for the
    /// current day. Note that habits recur, so a stale `done` row from
    /// yesterday will not match — only the current cycle counts.
    #[must_use]
    pub fn is_completed_today(&self, today: NaiveDate) -> bool {
        self.is_done() && self.completed_date == Some(today)
    }

    #[must_use]
    pub fn is_mit(&self) -> bool {
        self.types.iter().any(|t| t == "mit")
    }

    /// True iff this row is parked in the backlog (`status == "backlog"`).
    /// Backlog tasks are surfaced only in the Backlog and All views.
    #[must_use]
    pub fn is_backlog(&self) -> bool {
        self.status == "backlog"
    }

    #[must_use]
    pub fn is_past_due(&self, today: NaiveDate) -> bool {
        !self.is_done() && self.due_date.is_some_and(|d| d < today)
    }

    #[must_use]
    pub fn is_deferred(&self, today: NaiveDate) -> bool {
        self.start_date.is_some_and(|d| d > today)
    }

    #[must_use]
    pub fn is_stale(&self, today: NaiveDate) -> bool {
        !self.is_done()
            && self
                .last_touched
                .is_some_and(|d| (today - d).num_days() >= 21)
    }

    /// True iff this task carries a non-blank Linear issue identifier.
    #[must_use]
    pub fn has_linear(&self) -> bool {
        !self.linear_issue.trim().is_empty()
    }

    /// Full Linear issue URL for this task, or `None` when it carries no
    /// identifier or no workspace is configured. `base` is the workspace issue
    /// prefix (e.g. `https://linear.app/acme/issue/`); an empty `base` means no
    /// Linear workspace is set, so no link is produced.
    #[must_use]
    pub fn linear_url(&self, base: &str) -> Option<String> {
        let id = self.linear_issue.trim();
        if id.is_empty() || base.is_empty() {
            return None;
        }
        Some(format!("{base}{id}"))
    }

    #[must_use]
    pub fn matches_search(&self, q: &str) -> bool {
        let q = q.to_ascii_lowercase();
        self.name.to_ascii_lowercase().contains(&q)
            || self.notes.to_ascii_lowercase().contains(&q)
            || self.project.to_ascii_lowercase().contains(&q)
            || self.id.to_ascii_lowercase().contains(&q)
    }
}

#[cfg(test)]
#[must_use]
pub fn test_task(id: &str, status: &str) -> Task {
    Task {
        task_uuid: None,
        id: id.to_owned(),
        name: format!("test task {id}"),
        types: Vec::new(),
        status: status.to_owned(),
        priority: "p2".to_owned(),
        due_date: None,
        hard_deadline: false,
        start_date: None,
        assigned_to: String::new(),
        notes: String::new(),
        project: String::new(),
        energy: String::new(),
        context: String::new(),
        estimated_duration: None,
        defer_count: 0,
        last_touched: None,
        see_also: String::new(),
        blocked_by: Vec::new(),
        completed_date: None,
        linear_issue: String::new(),
        system_key: String::new(),
    }
}

#[cfg(test)]
mod tests;
