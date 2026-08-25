//! `brain tasks chronic|stale-waiting|linked` — the read-only triage scans.
//!
//! Each prints a themed table by default and JSON Lines with `--json`, so a
//! person and an agent read the same scan without either one parsing prose.

use anyhow::Result;
use chrono::Local;
use serde::Serialize;

use crate::tasks::cli::{LinkedArgs, ScanArgs, WaitingArgs};
use crate::tasks::complete::read_csv;
use crate::tasks::scan::{chronic, linked, waiting};
use crate::theme::Theme;
use crate::workspace::CommandContext;

fn rows(context: &CommandContext) -> Result<Vec<crate::tasks::complete::Row>> {
    Ok(read_csv(&context.workspace.root().join("tasks/tasks.csv"))?.rows)
}

/// Emit `hits` as JSON Lines, a count, or `render`'s table.
fn emit<T: Serialize>(
    hits: &[T],
    args: &ScanArgs,
    render: impl FnOnce(&[T]) -> String,
) -> Result<()> {
    if args.count {
        println!("{}", hits.len());
        return Ok(());
    }
    if args.json {
        for hit in hits {
            println!("{}", serde_json::to_string(hit)?);
        }
        return Ok(());
    }
    eprint!("{}", render(hits));
    Ok(())
}

pub(super) fn run_chronic(context: &CommandContext, args: &ScanArgs) -> Result<()> {
    let hits = chronic::scan(&rows(context)?, Local::now().date_naive());
    emit(&hits, args, |hits| render_chronic(hits, Theme::active()))
}

pub(super) fn run_waiting(context: &CommandContext, args: &WaitingArgs) -> Result<()> {
    let hits = waiting::scan(&rows(context)?, Local::now().date_naive(), args.threshold);
    emit(&hits, &args.scan, |hits| {
        render_waiting(hits, Theme::active())
    })
}

pub(super) fn run_linked(context: &CommandContext, args: &LinkedArgs) -> Result<()> {
    let hits = linked::scan(&rows(context)?, args.open_only);
    emit(&hits, &args.scan, |hits| {
        render_linked(hits, Theme::active())
    })
}

fn empty(label: &str, theme: Theme) -> String {
    format!("{}\n", theme.muted(label))
}

fn render_chronic(hits: &[chronic::ChronicTask], theme: Theme) -> String {
    use std::fmt::Write as _;

    if hits.is_empty() {
        return empty("Nothing is chronically ignored.", theme);
    }
    let mut out = format!(
        "{}\n",
        theme.heading(&format!("Chronically ignored — {} task(s)", hits.len()))
    );
    for hit in hits {
        let _ = writeln!(
            out,
            "  {:>6}  {}  {}  {}",
            theme.accent(&hit.task_id),
            theme.muted(&hit.priority),
            theme.value(&hit.task_name),
            theme.muted(&format!(
                "[{}{}]",
                hit.reasons.join(", "),
                hit.days_since_touch
                    .map_or_else(String::new, |days| format!("; {days}d untouched"))
            ))
        );
    }
    out
}

fn render_waiting(hits: &[waiting::StaleWait], theme: Theme) -> String {
    use std::fmt::Write as _;

    if hits.is_empty() {
        return empty("Nothing has been waiting too long.", theme);
    }
    let mut out = format!(
        "{}\n",
        theme.heading(&format!("Waiting too long — {} task(s)", hits.len()))
    );
    for hit in hits {
        let age = hit
            .days_waiting
            .map_or_else(|| "unknown".to_owned(), |days| format!("{days}d"));
        let _ = writeln!(
            out,
            "  {:>6}  {:>8}  {}",
            theme.accent(&hit.task_id),
            theme.muted(&age),
            theme.value(&hit.task_name)
        );
    }
    out
}

fn render_linked(hits: &[linked::LinkedTask], theme: Theme) -> String {
    use std::fmt::Write as _;

    if hits.is_empty() {
        return empty("No tasks carry a tracker link.", theme);
    }
    let mut out = format!(
        "{}\n",
        theme.heading(&format!("Tracker-linked — {} task(s)", hits.len()))
    );
    for hit in hits {
        let _ = writeln!(
            out,
            "  {:>6}  {:<10}  {}  {}",
            theme.accent(&hit.task_id),
            theme.value(&hit.linear_issue),
            theme.muted(&format!("[{}]", hit.status)),
            theme.value(&hit.task_name)
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{render_chronic, render_linked, render_waiting};
    use crate::tasks::scan::{chronic::ChronicTask, linked::LinkedTask, waiting::StaleWait};
    use crate::theme::Theme;

    fn plain() -> Theme {
        Theme::dark(false)
    }

    #[test]
    fn an_empty_scan_says_so_rather_than_printing_a_header() {
        assert_eq!(
            render_chronic(&[], plain()),
            "Nothing is chronically ignored.\n"
        );
        assert_eq!(
            render_waiting(&[], plain()),
            "Nothing has been waiting too long.\n"
        );
        assert_eq!(
            render_linked(&[], plain()),
            "No tasks carry a tracker link.\n"
        );
    }

    #[test]
    fn a_chronic_row_shows_every_reason_that_fired() {
        let hit = ChronicTask {
            task_id: "T1".to_owned(),
            task_name: "Call the vet".to_owned(),
            reasons: vec!["stale_21d", "stuck_in_progress"],
            days_since_touch: Some(30),
            days_since_create: Some(60),
            status: "in_progress".to_owned(),
            priority: "p2".to_owned(),
            task_type: String::new(),
            due_date: String::new(),
            defer_count: 0,
            project: String::new(),
            hard_deadline: false,
        };
        let out = render_chronic(&[hit], plain());
        assert!(out.contains("stale_21d, stuck_in_progress"), "{out}");
        assert!(out.contains("30d untouched"), "{out}");
    }

    #[test]
    fn an_unknown_wait_reads_as_unknown_not_as_zero() {
        let hit = StaleWait {
            task_id: "T1".to_owned(),
            task_name: "Chase".to_owned(),
            days_waiting: None,
            waiting_since: String::new(),
            priority: "p2".to_owned(),
            task_type: String::new(),
            due_date: String::new(),
            see_also: String::new(),
            notes: String::new(),
        };
        assert!(render_waiting(&[hit], plain()).contains("unknown"));
    }

    #[test]
    fn a_linked_row_leads_with_the_tracker_id() {
        let hit = LinkedTask {
            task_id: "T1".to_owned(),
            task_name: "Ship".to_owned(),
            status: "not_started".to_owned(),
            linear_issue: "ENG-7".to_owned(),
            task_type: String::new(),
            priority: "p1".to_owned(),
            project: String::new(),
        };
        let out = render_linked(&[hit], plain());
        assert!(out.contains("ENG-7"), "{out}");
        assert!(out.contains("[not_started]"), "{out}");
    }
}
