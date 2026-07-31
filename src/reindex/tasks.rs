//! The `--tasks` half of a reindex: apply the shared `/todo` automation rules
//! and habit cleanup.
//!
//! The rule set is owned by the `/todo` skill's Python scripts (the canonical,
//! shared implementation). brain shells out to the installed copies rather than
//! re-deriving the rules in Rust. Those scripts target the default `~/brain`
//! root, so a non-default configured root is reported as skipped (use
//! `/todo reindex` there instead).

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::skills::layout::Layout;
use crate::theme::Theme;

/// What happened (or didn't) for the task/habit portion of a reindex.
#[derive(Debug, PartialEq, Eq)]
pub enum TaskOutcome {
    Ran,
    ScriptsMissing(PathBuf),
    RootMismatch(PathBuf),
}

/// Decide whether we can run the todo scripts, and where they live. Errors carry
/// the outcome to report. Kept separate from the run so the decision is clear.
fn plan(root: &Path, home: &Path) -> Result<PathBuf, TaskOutcome> {
    let default_root = home.join("brain");
    if root != default_root {
        return Err(TaskOutcome::RootMismatch(default_root));
    }
    let scripts = Layout::real(home)
        .built_dir
        .join("todo")
        .join("scripts");
    if scripts.join("apply_sync_rules.py").exists() {
        Ok(scripts)
    } else {
        Err(TaskOutcome::ScriptsMissing(scripts))
    }
}

/// Apply task automation rules + habit cleanup by shelling out to the installed
/// `/todo` scripts. Their own output is inherited so the user sees what changed.
pub fn reindex_tasks(root: &Path, home: &Path) -> TaskOutcome {
    match plan(root, home) {
        Ok(scripts) => {
            run_py(&scripts.join("apply_sync_rules.py"), &["--fix"]);
            run_py(&scripts.join("cleanup_done_habits.py"), &[]);
            TaskOutcome::Ran
        }
        Err(outcome) => outcome,
    }
}

fn run_py(script: &Path, args: &[&str]) {
    let _ = Command::new("python3").arg(script).args(args).status();
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
        TaskOutcome::RootMismatch(default) => theme.warning(&format!(
            "skipped tasks: the todo rule scripts target the default root ({}); run `/todo reindex` for a non-default brain root",
            default.display()
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

    #[test]
    fn root_mismatch_points_at_todo_reindex() {
        let line = format_task_outcome(
            &TaskOutcome::RootMismatch(PathBuf::from("/home/u/brain")),
            Theme::dark(false),
        );
        assert!(line.contains("/todo reindex"), "{line:?}");
    }
}
