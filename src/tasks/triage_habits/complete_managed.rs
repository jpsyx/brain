//! Deterministic completion of a Brain-managed triage occurrence.
//!
//! "Skip daily triage today" is not a judgement call: it just marks today's
//! protected Morning Triage occurrence `done` and spawns the next one, exactly
//! like completion. The daily-triage modal's **Skip** button and
//! `brain habits complete-managed-triage <daily|weekly>` both run this, so the
//! action never has to round-trip through the brain panel / an agent.
//!
//! This selects the occurrence by stable `system_key` rather than by id, so no
//! caller has to know which id the current cycle carries. Completing the same
//! row by id through the ordinary paths is equally allowed — being managed
//! protects a chain from removal, revival, and manual skipping, not from being
//! ticked off. When the feature is **off**, this is a pure no-op (`Disabled`):
//! the day is acknowledged handled and no habit row is read or written, so a
//! fork with the feature disabled behaves identically.

use std::path::Path;

use anyhow::{Result, anyhow, bail};
use chrono::{Local, NaiveDate};

use super::model::{DAILY_SYSTEM_KEY, WEEKLY_SYSTEM_KEY};
use crate::tasks::complete::{CsvFile, field, read_csv, spawn_next_occurrence, write_csv};
use crate::theme::Theme;

/// Which managed triage chain to complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagedTriageKind {
    Daily,
    Weekly,
}

impl ManagedTriageKind {
    #[must_use]
    pub fn system_key(self) -> &'static str {
        match self {
            Self::Daily => DAILY_SYSTEM_KEY,
            Self::Weekly => WEEKLY_SYSTEM_KEY,
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Weekly => "weekly",
        }
    }
}

/// Outcome of a deterministic managed-triage completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManagedTriageCompletion {
    /// `enable_triage_habits` was off: the day is acknowledged handled and no
    /// habit row was read or mutated.
    Disabled,
    /// The pending occurrence was marked done and the next one spawned.
    Completed {
        task_id: String,
        next_id: String,
        next_due: String,
    },
}

/// Acquire the task-store lock and complete the managed occurrence for `today`.
pub fn complete_managed_triage(
    workspace: &crate::workspace::WorkspaceContext,
    kind: ManagedTriageKind,
    enabled: bool,
    today: NaiveDate,
) -> Result<ManagedTriageCompletion> {
    let _owner = crate::tasks::store_lock::TaskStoreOwner::acquire(workspace)?;
    complete_in_root(workspace.root(), kind, enabled, today)
}

/// CLI runner for `brain habits complete-managed-triage <daily|weekly>`.
pub fn run(workspace: &crate::workspace::WorkspaceContext, kind: ManagedTriageKind) -> Result<()> {
    crate::logging::log(format!(
        "habits complete-managed-triage kind={}",
        kind.label()
    ));
    let enabled = crate::config::Config::load(workspace).enable_triage_habits;
    let today = Local::now().date_naive();
    let outcome = complete_managed_triage(workspace, kind, enabled, today)?;
    print_outcome(kind, &outcome);
    Ok(())
}

pub(crate) fn complete_in_root(
    root: &Path,
    kind: ManagedTriageKind,
    enabled: bool,
    today: NaiveDate,
) -> Result<ManagedTriageCompletion> {
    if !enabled {
        return Ok(ManagedTriageCompletion::Disabled);
    }

    let tasks_dir = root.join("tasks");
    let habits_path = tasks_dir.join("habits.csv");
    let mut habits = read_csv(&habits_path)?;

    let idx = locate_pending(&habits, kind.system_key())?;

    let today_s = today.to_string();
    let task_id = {
        let row = habits
            .rows
            .get_mut(idx)
            .ok_or_else(|| anyhow!("habit row disappeared"))?;
        row.insert("status".to_owned(), "done".to_owned());
        row.insert("completed_date".to_owned(), today_s.clone());
        row.insert("last_touched".to_owned(), today_s);
        field(row, "task_id")
    };
    let (next_id, next_due) = spawn_next_occurrence(&tasks_dir, &mut habits, idx, today)?;
    write_csv(&habits_path, &habits)?;

    Ok(ManagedTriageCompletion::Completed {
        task_id,
        next_id,
        next_due,
    })
}

/// Find the single pending (not-`done`) occurrence of the managed chain.
///
/// Matching on the stable `system_key` (not a name or an id that changes each
/// cycle) is what makes this deterministic. Exactly one pending occurrence is
/// the invariant the reconcile step maintains; zero or several means the store
/// drifted, so we refuse rather than guess and point at the fix.
fn locate_pending(habits: &CsvFile, system_key: &str) -> Result<usize> {
    let mut found = None;
    for (idx, row) in habits.rows.iter().enumerate() {
        if field(row, "system_key").trim() == system_key && field(row, "status").trim() != "done" {
            if found.is_some() {
                bail!(
                    "expected exactly one pending managed triage habit for {system_key}; \
                     run `brain reindex --tasks` to reconcile definitions"
                );
            }
            found = Some(idx);
        }
    }
    found.ok_or_else(|| {
        anyhow!(
            "no pending managed triage habit for {system_key}; \
             run `brain reindex --tasks` to reconcile definitions"
        )
    })
}

fn print_outcome(kind: ManagedTriageKind, outcome: &ManagedTriageCompletion) {
    let theme = Theme::active();
    match outcome {
        ManagedTriageCompletion::Disabled => {
            eprintln!(
                "{} {}",
                theme.muted("managed triage habits disabled; day acknowledged, nothing mutated:"),
                theme.value(kind.label())
            );
        }
        ManagedTriageCompletion::Completed {
            task_id,
            next_id,
            next_due,
        } => {
            eprintln!(
                "{} {}  {}  {}",
                theme.success("triage marked done:"),
                theme.accent(task_id),
                theme.muted(kind.label()),
                theme.muted("(managed habit)")
            );
            eprintln!(
                "  {} {} {} {}",
                theme.info("next occurrence:"),
                theme.accent(next_id),
                theme.muted("due"),
                theme.value(next_due)
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ManagedTriageCompletion, ManagedTriageKind, complete_in_root};
    use chrono::NaiveDate;

    const HEADER: &str = "task_uuid,task_id,task_name,status,due_date,recur_interval,recur_unit,ideal_time,created_date,completed_date,last_touched,assigned_to,system_key";

    fn fixture(rows: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let tasks_dir = dir.path().join("tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("tasks.csv"),
            "task_id,task_name,status,completed_date,last_touched\n",
        )
        .unwrap();
        std::fs::write(tasks_dir.join("habits.csv"), format!("{HEADER}\n{rows}")).unwrap();
        dir
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 8, 4).unwrap()
    }

    #[test]
    fn daily_completion_marks_done_and_spawns_next() {
        let dir = fixture(
            "u1,H35,Morning Triage,not_started,2026-08-04,1,days,09:00,2026-08-04,,,pablo,brain.triage.daily\n",
        );
        let outcome =
            complete_in_root(dir.path(), ManagedTriageKind::Daily, true, today()).unwrap();

        match outcome {
            ManagedTriageCompletion::Completed {
                task_id, next_due, ..
            } => {
                assert_eq!(task_id, "H35");
                assert_eq!(next_due, "2026-08-05");
            }
            ManagedTriageCompletion::Disabled => panic!("expected Completed, got Disabled"),
        }

        let csv = std::fs::read_to_string(dir.path().join("tasks/habits.csv")).unwrap();
        assert!(
            csv.contains("H35,Morning Triage,done,2026-08-04"),
            "today's occurrence not completed; got:\n{csv}"
        );
        assert!(
            csv.contains("Morning Triage,not_started,2026-08-05,1,days"),
            "next occurrence not spawned; got:\n{csv}"
        );
    }

    #[test]
    fn disabled_is_a_pure_no_op() {
        let dir = fixture(
            "u1,H35,Morning Triage,not_started,2026-08-04,1,days,09:00,2026-08-04,,,pablo,brain.triage.daily\n",
        );
        let before = std::fs::read_to_string(dir.path().join("tasks/habits.csv")).unwrap();

        let outcome =
            complete_in_root(dir.path(), ManagedTriageKind::Daily, false, today()).unwrap();

        assert_eq!(outcome, ManagedTriageCompletion::Disabled);
        let after = std::fs::read_to_string(dir.path().join("tasks/habits.csv")).unwrap();
        assert_eq!(before, after, "disabled must not touch habits.csv");
    }

    #[test]
    fn weekly_completion_targets_the_weekly_chain_only() {
        let dir = fixture(
            "u1,H35,Morning Triage,not_started,2026-08-04,1,days,09:00,2026-08-04,,,pablo,brain.triage.daily\n\
             u2,H36,Weekly in-basket processing,not_started,2026-08-04,1,weeks,,2026-07-28,,,pablo,brain.triage.weekly\n",
        );
        let outcome =
            complete_in_root(dir.path(), ManagedTriageKind::Weekly, true, today()).unwrap();

        match outcome {
            ManagedTriageCompletion::Completed { task_id, .. } => assert_eq!(task_id, "H36"),
            ManagedTriageCompletion::Disabled => panic!("expected Completed, got Disabled"),
        }

        let csv = std::fs::read_to_string(dir.path().join("tasks/habits.csv")).unwrap();
        // The daily chain is untouched…
        assert!(csv.contains("H35,Morning Triage,not_started,2026-08-04"));
        // …only the weekly occurrence is completed.
        assert!(csv.contains("H36,Weekly in-basket processing,done,2026-08-04"));
    }

    #[test]
    fn already_completed_today_has_no_pending_occurrence() {
        // Today's occurrence is done and tomorrow's is pending — completing
        // again is a drift error, not a second completion of today.
        let dir = fixture(
            "u1,H35,Morning Triage,done,2026-08-04,1,days,09:00,2026-08-04,2026-08-04,2026-08-04,pablo,brain.triage.daily\n",
        );
        let err =
            complete_in_root(dir.path(), ManagedTriageKind::Daily, true, today()).unwrap_err();
        assert!(err.to_string().contains("no pending"), "got: {err}");
    }

    #[test]
    fn two_pending_occurrences_are_refused() {
        let dir = fixture(
            "u1,H35,Morning Triage,not_started,2026-08-04,1,days,09:00,2026-08-04,,,pablo,brain.triage.daily\n\
             u2,H40,Morning Triage,not_started,2026-08-05,1,days,09:00,2026-08-04,,,pablo,brain.triage.daily\n",
        );
        let err =
            complete_in_root(dir.path(), ManagedTriageKind::Daily, true, today()).unwrap_err();
        assert!(err.to_string().contains("exactly one"), "got: {err}");
    }
}
