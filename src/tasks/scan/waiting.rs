//! Tasks stuck waiting on someone else for too long.
//!
//! Waiting is not a failure — it is paused on an external party, which is why
//! deferring it carries no penalty. But waiting *forever* is its own failure
//! mode, and at some point the right move is to chase.
//!
//! A row with no `waiting_since` at all is surfaced regardless: not knowing how
//! long something has been waiting is itself the problem.

use chrono::NaiveDate;
use serde::Serialize;

use crate::tasks::complete::{Row, field, parse_date};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct StaleWait {
    pub(crate) task_id: String,
    pub(crate) task_name: String,
    pub(crate) days_waiting: Option<i64>,
    pub(crate) waiting_since: String,
    pub(crate) priority: String,
    pub(crate) task_type: String,
    pub(crate) due_date: String,
    pub(crate) see_also: String,
    pub(crate) notes: String,
}

pub(crate) fn classify(row: &Row, today: NaiveDate, threshold: i64) -> Option<StaleWait> {
    if field(row, "status").trim() != "waiting" {
        return None;
    }
    let waiting_since = field(row, "waiting_since");
    let days_waiting = parse_date(&waiting_since).map(|since| (today - since).num_days());
    if days_waiting.is_some_and(|days| days <= threshold) {
        return None;
    }
    Some(StaleWait {
        task_id: field(row, "task_id"),
        task_name: field(row, "task_name"),
        days_waiting,
        waiting_since,
        priority: field(row, "priority"),
        task_type: field(row, "task_type"),
        due_date: field(row, "due_date"),
        see_also: field(row, "see_also"),
        notes: field(row, "notes"),
    })
}

/// Longest wait first; rows with no recorded start sort last.
pub(crate) fn scan(rows: &[Row], today: NaiveDate, threshold: i64) -> Vec<StaleWait> {
    let mut hits: Vec<StaleWait> = rows
        .iter()
        .filter_map(|row| classify(row, today, threshold))
        .collect();
    hits.sort_by(|left, right| {
        left.days_waiting
            .is_none()
            .cmp(&right.days_waiting.is_none())
            .then_with(|| {
                right
                    .days_waiting
                    .unwrap_or(0)
                    .cmp(&left.days_waiting.unwrap_or(0))
            })
            .then_with(|| left.task_id.cmp(&right.task_id))
    });
    hits
}
