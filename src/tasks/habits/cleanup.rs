//! Retention for completed habit occurrences.
//!
//! Every completion leaves a row behind, so `habits.csv` grows forever without
//! a sweep. Completed occurrences stay for a week — long enough to inspect or
//! undo — and are then dropped.
//!
//! **Managed triage rows are never swept here.** Removing one is a
//! transactional decision that belongs to `brain config set
//! enable_triage_habits=false`, which stages config, both CSVs, and every
//! derived reference together. A retention sweep quietly deleting half of that
//! would leave the reconciler with nothing to maintain.

use chrono::NaiveDate;

use crate::tasks::complete::{Row, field, parse_date};

/// How long a completed occurrence stays before it is swept.
pub(crate) const RETENTION_DAYS: u64 = 7;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CleanupPlan {
    /// Rows to drop, in file order.
    pub(crate) dropped: Vec<String>,
    pub(crate) kept: usize,
    pub(crate) cutoff: NaiveDate,
    /// Managed rows held back because the feature is off but their removal is
    /// transactional.
    pub(crate) deferred_managed: usize,
}

/// Pure: which completed occurrences are past the retention window.
pub(crate) fn plan(rows: &[Row], today: NaiveDate, triage_enabled: bool) -> CleanupPlan {
    let cutoff = today - chrono::Days::new(RETENTION_DAYS);
    let mut dropped = Vec::new();
    let mut kept = 0;
    let mut deferred_managed = 0;
    for row in rows {
        if crate::tasks::triage_habits::is_managed_system_key(&field(row, "system_key")) {
            kept += 1;
            if !triage_enabled {
                deferred_managed += 1;
            }
            continue;
        }
        let expired = field(row, "status").trim() == "done"
            && parse_date(&field(row, "completed_date")).is_some_and(|done| done <= cutoff);
        if expired {
            dropped.push(field(row, "task_id"));
        } else {
            kept += 1;
        }
    }
    CleanupPlan {
        dropped,
        kept,
        cutoff,
        deferred_managed,
    }
}

/// Apply the plan and render a report.
pub(crate) fn run_in_root(
    root: &std::path::Path,
    today: NaiveDate,
    triage_enabled: bool,
) -> anyhow::Result<String> {
    use crate::tasks::complete::{read_csv, write_csv};

    let path = root.join("tasks/habits.csv");
    let mut csv = read_csv(&path)?;
    let plan = plan(&csv.rows, today, triage_enabled);
    if !plan.dropped.is_empty() {
        csv.rows
            .retain(|row| !plan.dropped.contains(&field(row, "task_id")));
        write_csv(&path, &csv)?;
    }
    Ok(render(&plan, crate::theme::Theme::active()))
}

/// Pure: what the sweep says it did.
pub(crate) fn render(plan: &CleanupPlan, theme: crate::theme::Theme) -> String {
    use std::fmt::Write as _;

    let mut out = if plan.dropped.is_empty() {
        format!(
            "{}\n",
            theme.muted(&format!(
                "No completed habits older than {}; kept {} row(s).",
                plan.cutoff, plan.kept
            ))
        )
    } else {
        format!(
            "{}\n",
            theme.success(&format!(
                "Swept {} completed habit(s) older than {}; kept {} row(s).",
                plan.dropped.len(),
                plan.cutoff,
                plan.kept
            ))
        )
    };
    if plan.deferred_managed > 0 {
        let _ = writeln!(
            out,
            "  {}",
            theme.muted(&format!(
                "managed triage rows are removed transactionally; run `{}`",
                crate::workspace::suggest("config set enable_triage_habits=false")
            ))
        );
    }
    out
}
