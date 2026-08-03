//! The `--tasks` half of a reindex: apply the shared `/todo` automation rules
//! and habit cleanup.
//!
//! The rule set is owned by the `/todo` skill's Python scripts (the canonical,
//! shared implementation). brain shells out to the installed copies rather than
//! re-deriving the rules in Rust. Brain passes the selected workspace identity
//! and root through the standard integration environment.

use std::path::{Path, PathBuf};
use std::process::Command;

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
fn plan(home: &Path) -> Result<PathBuf, TaskOutcome> {
    let scripts = Layout::real(home).built_dir.join("todo").join("scripts");
    if scripts.join("apply_sync_rules.py").exists() {
        Ok(scripts)
    } else {
        Err(TaskOutcome::ScriptsMissing(scripts))
    }
}

/// Apply task automation rules + habit cleanup by shelling out to the installed
/// `/todo` scripts. Their own output is inherited so the user sees what changed.
pub fn reindex_tasks(workspace: &crate::workspace::WorkspaceContext, home: &Path) -> TaskOutcome {
    match plan(home) {
        Ok(scripts) => {
            run_py(workspace, &scripts.join("apply_sync_rules.py"), &["--fix"]);
            run_py(workspace, &scripts.join("cleanup_done_habits.py"), &[]);
            TaskOutcome::Ran
        }
        Err(outcome) => outcome,
    }
}

fn run_py(workspace: &crate::workspace::WorkspaceContext, script: &Path, args: &[&str]) {
    if let Ok(actor) = crate::actor::local_actor(workspace) {
        let _ = Command::new("python3")
            .arg(script)
            .args(args)
            .envs(workspace.integration_env(&actor))
            .status();
    }
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
}
