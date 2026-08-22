//! The `--tasks` half of a reindex: apply the shared `/todo` automation rules
//! and habit cleanup.
//!
//! The rule set is owned by the `/todo` skill's Python scripts (the canonical,
//! shared implementation). brain shells out to the installed copies rather than
//! re-deriving the rules in Rust. Brain passes the selected workspace identity
//! and root through the standard integration environment.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::skills::layout::Layout;
use crate::theme::Theme;

/// What happened (or didn't) for the task/habit portion of a reindex.
#[derive(Debug, PartialEq, Eq)]
pub enum TaskOutcome {
    Ran,
    ScriptsMissing(PathBuf),
}

/// Decide whether we can run the todo scripts, and where they live. Errors carry
/// the outcome to report. Kept separate from the run so the decision is clear.
fn plan(root: &Path) -> Result<PathBuf, TaskOutcome> {
    let scripts = Layout::real(root).built_dir.join("todo").join("scripts");
    if scripts.join("apply_sync_rules.py").exists() {
        Ok(scripts)
    } else {
        Err(TaskOutcome::ScriptsMissing(scripts))
    }
}

fn reconcile_triage_before_reindex(workspace: &crate::workspace::WorkspaceContext) -> Result<()> {
    let enabled = crate::config::Config::load(workspace).enable_triage_habits;
    crate::tasks::triage_habits::apply_triage_habits_config(workspace, enabled)
}

/// Apply task automation rules + habit cleanup by shelling out to the installed
/// `/todo` scripts. Their own output is inherited so the user sees what changed.
pub fn reindex_tasks(
    workspace: &crate::workspace::WorkspaceContext,
    actor: &crate::actor::ActorContext,
    _home: &Path,
) -> Result<TaskOutcome> {
    reconcile_triage_before_reindex(workspace)?;
    match plan(workspace.root()) {
        Ok(scripts) => {
            run_py(
                workspace,
                actor,
                &scripts.join("apply_sync_rules.py"),
                &["--fix"],
            )?;
            run_py(
                workspace,
                actor,
                &scripts.join("cleanup_done_habits.py"),
                &[],
            )?;
            Ok(TaskOutcome::Ran)
        }
        Err(outcome) => Ok(outcome),
    }
}

fn run_py(
    workspace: &crate::workspace::WorkspaceContext,
    actor: &crate::actor::ActorContext,
    script: &Path,
    args: &[&str],
) -> Result<()> {
    let status = Command::new("python3")
        .arg(script)
        .args(args)
        .envs(workspace.integration_env(actor))
        .status()
        .with_context(|| format!("run {}", script.display()))?;
    if !status.success() {
        bail!("{} exited with {status}", script.display());
    }
    Ok(())
}

/// The themed report line for a task outcome. Pure.
#[must_use]
pub fn format_task_outcome(outcome: &TaskOutcome, theme: Theme) -> String {
    match outcome {
        TaskOutcome::Ran => {
            theme.success("✓ tasks + habits: applied automation rules and habit cleanup")
        }
        TaskOutcome::ScriptsMissing(path) => theme.warning(&format!(
            "skipped tasks: todo scripts not found at {} — run `brain skills sync`",
            path.display()
        )),
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
    fn scripts_missing_points_at_skills_sync() {
        let line = format_task_outcome(
            &TaskOutcome::ScriptsMissing(PathBuf::from("/x/todo/scripts")),
            Theme::dark(false),
        );
        assert!(line.contains("brain skills sync"), "{line:?}");
    }

    #[test]
    fn child_scripts_receive_the_request_boundary_actor_in_legacy_workspaces() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = legacy_workspace(&temp);
        let actor = crate::actor::local_actor(&workspace).unwrap();
        let script = temp.path().join("record_actor.py");
        let output = temp.path().join("actor.txt");
        std::fs::write(
            &script,
            "import os, pathlib, sys\npathlib.Path(sys.argv[1]).write_text(os.environ['BRAIN_ACTOR_ID'])\n",
        )
        .unwrap();

        run_py(&workspace, &actor, &script, &[output.to_str().unwrap()]).unwrap();

        assert_eq!(std::fs::read_to_string(output).unwrap(), "pablo");
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
}
