//! Chronically-ignored tasks: the deadwood sweep.
//!
//! A task qualifies when it is not done, not parked, actually actionable, its
//! deadline is imminent or absent, and at least one staleness signal fires.
//!
//! The horizon is deliberately tight. A dated task is only nagged about once
//! its due date is within three days — anything further out is not being
//! *ignored*, it is simply scheduled — and anything already past due belongs to
//! past-due triage, not here. An undated thin row has no deadline to be away
//! from, so it stays eligible: those are the truest captured-and-forgotten
//! rows, and the ones this sweep exists for.

use chrono::NaiveDate;
use serde::Serialize;

use crate::tasks::complete::{Row, field, parse_date};

/// Untouched for this long is the primary signal.
const STALE_TOUCH_DAYS: i64 = 21;
/// Started, then walked away from.
const STUCK_IN_PROGRESS_DAYS: i64 = 14;
/// Old, thin, and never started.
const CAPTURED_FORGOTTEN_AGE_DAYS: i64 = 60;
/// A dated task is left alone until its deadline is this close.
const DUE_HORIZON_DAYS: i64 = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ChronicTask {
    pub(crate) task_id: String,
    pub(crate) task_name: String,
    pub(crate) reasons: Vec<&'static str>,
    pub(crate) days_since_touch: Option<i64>,
    pub(crate) days_since_create: Option<i64>,
    pub(crate) status: String,
    pub(crate) priority: String,
    pub(crate) task_type: String,
    pub(crate) due_date: String,
    pub(crate) defer_count: u32,
    pub(crate) project: String,
    pub(crate) hard_deadline: bool,
}

fn days_since(value: &str, today: NaiveDate) -> Option<i64> {
    parse_date(value).map(|date| (today - date).num_days())
}

/// Pure: is this row chronically ignored, and why?
pub(crate) fn classify(row: &Row, today: NaiveDate) -> Option<ChronicTask> {
    let status = field(row, "status").trim().to_owned();
    if matches!(status.as_str(), "done" | "backlog") {
        return None;
    }
    // Deliberately hidden until its start date: it cannot be ignored yet.
    if parse_date(&field(row, "start_date")).is_some_and(|start| start > today) {
        return None;
    }
    if let Some(due) = parse_date(&field(row, "due_date")) {
        if due < today || due > today + chrono::Days::new(DUE_HORIZON_DAYS.unsigned_abs()) {
            return None;
        }
    }

    let days_since_touch = days_since(&field(row, "last_touched"), today);
    let days_since_create = days_since(&field(row, "created_date"), today);
    let mut reasons = Vec::new();
    if days_since_touch.is_some_and(|days| days >= STALE_TOUCH_DAYS) {
        reasons.push("stale_21d");
    }
    if status == "in_progress"
        && days_since_touch.is_some_and(|days| days >= STUCK_IN_PROGRESS_DAYS)
    {
        reasons.push("stuck_in_progress");
    }
    let thin = ["notes", "estimated_duration", "project"]
        .iter()
        .all(|column| field(row, column).trim().is_empty());
    if status == "not_started"
        && thin
        && days_since_create.is_some_and(|days| days >= CAPTURED_FORGOTTEN_AGE_DAYS)
    {
        reasons.push("captured_forgotten");
    }
    if reasons.is_empty() {
        return None;
    }

    Some(ChronicTask {
        task_id: field(row, "task_id"),
        task_name: field(row, "task_name"),
        reasons,
        days_since_touch,
        days_since_create,
        status,
        priority: field(row, "priority"),
        task_type: field(row, "task_type"),
        due_date: field(row, "due_date"),
        defer_count: field(row, "defer_count").trim().parse().unwrap_or(0),
        project: field(row, "project"),
        hard_deadline: field(row, "hard_deadline").trim() == "true",
    })
}

/// Every chronic row, worst first.
pub(crate) fn scan(rows: &[Row], today: NaiveDate) -> Vec<ChronicTask> {
    let mut hits: Vec<ChronicTask> = rows.iter().filter_map(|row| classify(row, today)).collect();
    hits.sort_by(|left, right| {
        right
            .days_since_touch
            .unwrap_or(0)
            .cmp(&left.days_since_touch.unwrap_or(0))
            .then_with(|| {
                right
                    .days_since_create
                    .unwrap_or(0)
                    .cmp(&left.days_since_create.unwrap_or(0))
            })
            .then_with(|| left.task_id.cmp(&right.task_id))
    });
    hits
}
