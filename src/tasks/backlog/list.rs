//! Listing the backlog for the monthly review.
//!
//! Sorted oldest-parked first and stamped with how long each has sat, because
//! staleness is the whole question the reviewer is answering. This only lists;
//! deciding what to resurface is the caller's.

use chrono::NaiveDate;
use serde::Serialize;

use crate::tasks::complete::{Row, field, parse_date};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct BacklogEntry {
    pub(crate) task_id: String,
    pub(crate) task_name: String,
    pub(crate) task_type: String,
    pub(crate) priority: String,
    pub(crate) project: String,
    pub(crate) backlogged_date: String,
    /// Days since it was parked, or `None` when it carries no parking date.
    pub(crate) days_in_backlog: Option<i64>,
    pub(crate) notes: String,
}

/// Pure: every parked row, oldest first.
pub(crate) fn entries(rows: &[Row], today: NaiveDate) -> Vec<BacklogEntry> {
    let mut entries: Vec<BacklogEntry> = rows
        .iter()
        .filter(|row| field(row, "status").trim() == "backlog")
        .map(|row| {
            let backlogged_date = field(row, "backlogged_date");
            BacklogEntry {
                task_id: field(row, "task_id"),
                task_name: field(row, "task_name"),
                task_type: field(row, "task_type"),
                priority: field(row, "priority"),
                project: field(row, "project"),
                days_in_backlog: parse_date(&backlogged_date)
                    .map(|parked| (today - parked).num_days()),
                backlogged_date,
                notes: field(row, "notes"),
            }
        })
        .collect();
    entries.sort_by(|left, right| {
        right
            .days_in_backlog
            .unwrap_or(0)
            .cmp(&left.days_in_backlog.unwrap_or(0))
            .then_with(|| left.task_id.cmp(&right.task_id))
    });
    entries
}
