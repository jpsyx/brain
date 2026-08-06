//! Canonical current-schema column ordering shared by migration and merge.

use std::collections::BTreeSet;

const KNOWN: [&str; 29] = [
    "task_uuid",
    "task_id",
    "task_name",
    "task_type",
    "status",
    "waiting_since",
    "priority",
    "due_date",
    "hard_deadline",
    "start_date",
    "assigned_to",
    "see_also",
    "notes",
    "project",
    "energy_level",
    "context",
    "estimated_duration",
    "blocked_by",
    "defer_count",
    "recur_interval",
    "recur_unit",
    "ideal_time",
    "created_date",
    "completed_date",
    "last_touched",
    "linear_issue",
    "system_key",
    "calendar_id",
    "waiting_for",
];

pub(crate) fn canonical_current_header(columns: &[String]) -> Vec<String> {
    let mut remaining = columns.iter().cloned().collect::<BTreeSet<_>>();
    let mut header = KNOWN
        .into_iter()
        .filter(|column| remaining.remove(*column))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    header.extend(remaining);
    header
}

pub(crate) fn is_known_current_column(column: &str) -> bool {
    KNOWN.contains(&column)
}
