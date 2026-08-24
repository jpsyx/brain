//! Deleting backlog tasks an active task has already superseded.
//!
//! If a parked task has a near-identical twin on the **active** list, and that
//! twin was created *after* the parking date, the user already re-created the
//! task by hand — they revived it themselves, and the parked copy is now a
//! stale duplicate.
//!
//! The date guard is the whole rule. A twin created *before* the parking means
//! the two merely coexisted, which is not an intentional re-creation, so the
//! parked task stays. Matching is deliberately conservative — names equal after
//! lowercasing, stripping punctuation, and collapsing whitespace — because a
//! false delete is far worse than a near-duplicate the caller can catch.

use std::collections::HashMap;

use chrono::NaiveDate;
use serde::Serialize;

use crate::tasks::complete::{Row, field, parse_date};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Superseded {
    pub(crate) task_id: String,
    pub(crate) task_name: String,
    pub(crate) backlogged_date: String,
}

/// Lowercase, drop punctuation, collapse whitespace.
pub(crate) fn normalize(name: &str) -> String {
    name.trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Pure: parked rows whose active twin was created after they were parked.
pub(crate) fn superseded(rows: &[Row]) -> Vec<Superseded> {
    let mut active_created: HashMap<String, NaiveDate> = HashMap::new();
    for row in rows {
        let status = field(row, "status");
        if matches!(status.trim(), "done" | "backlog") {
            continue;
        }
        let key = normalize(&field(row, "task_name"));
        let Some(created) = parse_date(&field(row, "created_date")) else {
            continue;
        };
        if key.is_empty() {
            continue;
        }
        active_created
            .entry(key)
            .and_modify(|latest| {
                if created > *latest {
                    *latest = created;
                }
            })
            .or_insert(created);
    }

    rows.iter()
        .filter(|row| field(row, "status").trim() == "backlog")
        .filter_map(|row| {
            let parked = parse_date(&field(row, "backlogged_date"))?;
            let twin = active_created.get(&normalize(&field(row, "task_name")))?;
            (*twin > parked).then(|| Superseded {
                task_id: field(row, "task_id"),
                task_name: field(row, "task_name"),
                backlogged_date: field(row, "backlogged_date"),
            })
        })
        .collect()
}
