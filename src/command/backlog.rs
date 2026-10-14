//! `brain backlog` — review the backlog, and run its two maintenance passes.
//!
//! The passes are silent in normal use: triage runs them plain, and nothing
//! reaches the user. `--dry-run` and `--report` are for a human checking the
//! rule, not for the routine pass.

use anyhow::Result;
use chrono::Local;

use crate::cli::{BacklogAction, BacklogArgs, BacklogMaintenanceArgs};
use crate::tasks::backlog::list;
use crate::tasks::complete::read_csv;
use crate::tasks::removal::maintenance::{MaintenanceKind, preview_maintenance};
use crate::theme::Theme;
use crate::workspace::CommandContext;

pub fn run(args: &BacklogArgs, context: &CommandContext) -> Result<()> {
    match &args.action {
        None => review(context, args.json),
        Some(BacklogAction::Park(move_args)) => {
            crate::command::tasks::run_backlog_move(context, &move_args.id, false)
        }
        Some(BacklogAction::Restore(move_args)) => {
            crate::command::tasks::run_backlog_move(context, &move_args.id, true)
        }
        Some(BacklogAction::Purge(maintenance)) => run_purge(context, maintenance),
        Some(BacklogAction::Dedupe(maintenance)) => run_dedupe(context, maintenance),
    }
}

fn tasks_path(context: &CommandContext) -> std::path::PathBuf {
    context.workspace.root().join("tasks/tasks.csv")
}

fn review(context: &CommandContext, json: bool) -> Result<()> {
    let csv = read_csv(&tasks_path(context))?;
    let entries = list::entries(&csv.rows, Local::now().date_naive());
    if json {
        for entry in &entries {
            println!("{}", serde_json::to_string(entry)?);
        }
        return Ok(());
    }
    eprint!("{}", render_review(&entries, Theme::active()));
    Ok(())
}

/// Pure: the human-facing review table.
fn render_review(entries: &[list::BacklogEntry], theme: Theme) -> String {
    use std::fmt::Write as _;

    if entries.is_empty() {
        return format!("{}\n", theme.muted("The backlog is empty."));
    }
    let mut out = format!(
        "{}\n",
        theme.heading(&format!(
            "Backlog — {} parked task{}",
            entries.len(),
            if entries.len() == 1 { "" } else { "s" }
        ))
    );
    for entry in entries {
        let age = entry
            .days_in_backlog
            .map_or_else(|| "  ?".to_owned(), |days| format!("{days}d"));
        let _ = writeln!(
            out,
            "  {:>6}  {}  {:>6}  {}{}",
            theme.accent(&entry.task_id),
            theme.muted(&entry.priority),
            theme.muted(&age),
            theme.value(&entry.task_name),
            if entry.project.trim().is_empty() {
                String::new()
            } else {
                format!("  {}", theme.muted(&format!("[{}]", entry.project)))
            }
        );
    }
    out
}

fn run_purge(context: &CommandContext, args: &BacklogMaintenanceArgs) -> Result<()> {
    run_maintenance(context, args, MaintenanceKind::Purge)
}
fn run_dedupe(context: &CommandContext, args: &BacklogMaintenanceArgs) -> Result<()> {
    run_maintenance(context, args, MaintenanceKind::Dedupe)
}
fn run_maintenance(
    context: &CommandContext,
    args: &BacklogMaintenanceArgs,
    kind: MaintenanceKind,
) -> Result<()> {
    let preview = preview_maintenance(context, kind, Local::now().date_naive())?;
    if args.dry_run {
        let mut report = serde_json::json!({"would_delete": match kind { MaintenanceKind::Purge => serde_json::to_value(&preview.purged)?, MaintenanceKind::Dedupe => serde_json::to_value(&preview.superseded)? }, "skipped": preview.skipped});
        if let Some(cutoff) = preview.cutoff {
            report["cutoff"] = cutoff.into();
        }
        if let Some(plan) = &preview.plan {
            report["blockers"] = serde_json::to_value(&plan.blockers)?;
        }
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    for skipped in &preview.skipped {
        crate::logging::log(format!(
            "Backlog dedupe {}: {}",
            skipped.task_id, skipped.code
        ));
    }
    let removed = if let Some(plan) = preview.plan {
        crate::tasks::removal::apply(context, &plan, &plan.fingerprint, Local::now().date_naive())?
            .removed
    } else {
        Vec::new()
    };
    if args.report {
        println!(
            "{}",
            serde_json::to_string(
                &serde_json::json!({"deleted_count": removed.len(), "deleted_ids": removed.iter().map(|task| &task.task_id).collect::<Vec<_>>(), "skipped": preview.skipped})
            )?
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::render_review;
    use crate::tasks::backlog::list::BacklogEntry;
    use crate::theme::Theme;

    fn entry(id: &str, days: Option<i64>, project: &str) -> BacklogEntry {
        BacklogEntry {
            task_id: id.to_owned(),
            task_name: "Call the vet".to_owned(),
            task_type: String::new(),
            priority: "p3".to_owned(),
            project: project.to_owned(),
            backlogged_date: "2025-01-01".to_owned(),
            days_in_backlog: days,
            notes: String::new(),
        }
    }

    #[test]
    fn an_empty_backlog_says_so_rather_than_printing_a_header() {
        assert_eq!(
            render_review(&[], Theme::dark(false)),
            "The backlog is empty.\n"
        );
    }

    #[test]
    fn the_review_shows_age_and_project() {
        let out = render_review(&[entry("T1", Some(600), "website")], Theme::dark(false));
        assert!(out.contains("Backlog — 1 parked task\n"), "{out}");
        assert!(out.contains("T1"), "{out}");
        assert!(out.contains("600d"), "{out}");
        assert!(out.contains("[website]"), "{out}");
    }

    #[test]
    fn a_row_with_no_parking_date_still_renders() {
        let out = render_review(&[entry("T1", None, "")], Theme::dark(false));
        assert!(out.contains('?'), "{out}");
    }
}

#[cfg(test)]
#[path = "backlog_cleanup_tests.rs"]
mod cleanup_tests;
