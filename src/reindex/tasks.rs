//! The `--tasks` half of a reindex: the task automation rules plus the
//! habit-retention sweep.
//!
//! Both halves are native. The rules live in [`crate::tasks::rules`] and the
//! completed-occurrence sweep in [`crate::tasks::habits::cleanup`], so a
//! reindex depends on nothing but the binary — no `python3`, no installed skill
//! scripts, and no second copy of the rules to drift from this one.

use std::path::Path;

use anyhow::Result;

use crate::theme::Theme;

/// What happened for the task/habit portion of a reindex.
#[derive(Debug, PartialEq, Eq)]
pub enum TaskOutcome {
    Ran,
}

fn reconcile_triage_before_reindex(workspace: &crate::workspace::WorkspaceContext) -> Result<()> {
    let enabled = crate::config::Config::load(workspace).enable_triage_habits;
    crate::tasks::triage_habits::apply_triage_habits_config(workspace, enabled)
}

/// Reconcile managed triage, apply every rule, then sweep old completions.
pub fn reindex_tasks(
    workspace: &crate::workspace::WorkspaceContext,
    _home: &Path,
) -> Result<TaskOutcome> {
    reconcile_triage_before_reindex(workspace)?;
    let today = chrono::Local::now().date_naive();
    let _owner = crate::tasks::store_lock::TaskStoreOwner::acquire(workspace)?;
    let theme = Theme::active();
    let report = crate::tasks::rules::run(workspace.root(), today, true)?;
    eprint!("{}", crate::tasks::rules::render(&report, true, theme));
    eprint!(
        "{}",
        crate::tasks::habits::cleanup::run_in_root(
            workspace.root(),
            today,
            crate::config::Config::load(workspace).enable_triage_habits,
        )?
    );
    Ok(TaskOutcome::Ran)
}

/// The themed report line for a task outcome. Pure.
#[must_use]
pub fn format_task_outcome(outcome: &TaskOutcome, theme: Theme) -> String {
    match outcome {
        TaskOutcome::Ran => {
            theme.success("✓ tasks + habits: applied automation rules and habit cleanup")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy_workspace(temp: &tempfile::TempDir) -> crate::workspace::WorkspaceContext {
        let root = temp.path().join("legacy-brain");
        std::fs::create_dir_all(&root).unwrap();
        crate::workspace::WorkspaceContext::new(
            temp.path(),
            crate::workspace::WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap(),
            crate::workspace::WorkspaceName::parse("legacy").unwrap(),
            &root,
            "pablo",
            temp.path(),
        )
        .unwrap()
    }

    #[test]
    fn ran_reports_success() {
        let line = format_task_outcome(&TaskOutcome::Ran, Theme::dark(false));
        assert!(line.contains("tasks + habits"), "{line:?}");
    }

    #[test]
    fn task_reindex_restores_missing_managed_triage_definitions() {
        let temporary = tempfile::tempdir().unwrap();
        let workspace = legacy_workspace(&temporary);
        std::fs::create_dir_all(workspace.root().join("tasks")).unwrap();
        std::fs::create_dir_all(workspace.root().join(".config")).unwrap();
        std::fs::write(
            workspace.root().join("tasks/tasks.csv"),
            "task_uuid,task_id,task_name,status,assigned_to,system_key\n",
        )
        .unwrap();
        std::fs::write(
            workspace.root().join("tasks/habits.csv"),
            "task_uuid,task_id,task_name,status,assigned_to,system_key\n",
        )
        .unwrap();
        std::fs::write(workspace.root().join(".config/config.json"), "{}\n").unwrap();

        reconcile_triage_before_reindex(&workspace).unwrap();

        let habits =
            crate::tasks::task::load_habits(&workspace.root().join("tasks/habits.csv")).unwrap();
        assert_eq!(
            habits
                .iter()
                .filter(|habit| habit.is_managed_triage())
                .count(),
            2
        );
    }

    /// The whole point of the port: a reindex must work on a machine with no
    /// installed skill scripts and no `python3`.
    #[test]
    fn the_task_reindex_needs_no_installed_scripts() {
        let temporary = tempfile::tempdir().unwrap();
        let workspace = legacy_workspace(&temporary);
        std::fs::create_dir_all(workspace.root().join("tasks")).unwrap();
        std::fs::create_dir_all(workspace.root().join(".config")).unwrap();
        std::fs::write(
            workspace.root().join("tasks/tasks.csv"),
            "task_id,task_name,status,completed_date,created_date\nT1,Ship,done,,2026-01-05\n",
        )
        .unwrap();
        std::fs::write(
            workspace.root().join("tasks/habits.csv"),
            "task_id,task_name,status,completed_date,created_date\n",
        )
        .unwrap();
        std::fs::write(
            workspace.root().join(".config/config.json"),
            "{\"enable_triage_habits\":false}\n",
        )
        .unwrap();

        assert_eq!(
            reindex_tasks(&workspace, temporary.path()).unwrap(),
            TaskOutcome::Ran
        );

        let tasks = std::fs::read_to_string(workspace.root().join("tasks/tasks.csv")).unwrap();
        assert!(tasks.contains("last_touched"), "{tasks}");
    }
}
