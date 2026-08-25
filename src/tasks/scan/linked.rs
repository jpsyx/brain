//! Tasks carrying an external issue-tracker link.
//!
//! Brain never contacts a tracker: `linear_issue` is inert link plumbing. This
//! is the structured read a caller reconciles *from* — it lists what is linked
//! and leaves the reconciling to whoever can reach the tracker.

use serde::Serialize;

use crate::tasks::complete::{Row, field};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct LinkedTask {
    pub(crate) task_id: String,
    pub(crate) task_name: String,
    pub(crate) status: String,
    pub(crate) linear_issue: String,
    pub(crate) task_type: String,
    pub(crate) priority: String,
    pub(crate) project: String,
}

pub(crate) fn scan(rows: &[Row], open_only: bool) -> Vec<LinkedTask> {
    rows.iter()
        .filter(|row| !field(row, "linear_issue").trim().is_empty())
        .filter(|row| !open_only || field(row, "status").trim() != "done")
        .map(|row| LinkedTask {
            task_id: field(row, "task_id"),
            task_name: field(row, "task_name"),
            status: field(row, "status"),
            linear_issue: field(row, "linear_issue"),
            task_type: field(row, "task_type"),
            priority: field(row, "priority"),
            project: field(row, "project"),
        })
        .collect()
}
