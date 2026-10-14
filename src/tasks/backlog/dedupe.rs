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

use std::collections::BTreeMap;

use chrono::NaiveDate;
use serde::Serialize;

use crate::tasks::complete::{Row, field, parse_date};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Superseded {
    pub(crate) task_id: String,
    pub(crate) task_name: String,
    pub(crate) backlogged_date: String,
    pub(crate) survivor_id: String,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Skipped {
    pub task_id: String,
    pub survivor_ids: Vec<String>,
    pub code: String,
}

#[cfg(test)]
pub(crate) fn superseded(rows: &[Row]) -> Vec<Superseded> {
    candidates(rows).0
}

/// Pure: exactly one newer active twin is evidence of recreation.
pub(crate) fn candidates(rows: &[Row]) -> (Vec<Superseded>, Vec<Skipped>) {
    let mut active: BTreeMap<String, Vec<(NaiveDate, String)>> = BTreeMap::new();
    for row in rows {
        if matches!(field(row, "status").trim(), "done" | "backlog") {
            continue;
        }
        let name = normalize(&field(row, "task_name"));
        if name.is_empty() {
            continue;
        }
        if let Some(date) = parse_date(&field(row, "created_date")) {
            active
                .entry(name)
                .or_default()
                .push((date, field(row, "task_id")));
        }
    }
    let mut selected = Vec::new();
    let mut skipped = Vec::new();
    for row in rows
        .iter()
        .filter(|row| field(row, "status").trim() == "backlog")
    {
        let Some(parked) = parse_date(&field(row, "backlogged_date")) else {
            continue;
        };
        let Some(twins) = active.get(&normalize(&field(row, "task_name"))) else {
            continue;
        };
        let mut eligible: Vec<_> = twins
            .iter()
            .filter(|(created, _)| *created > parked)
            .map(|(_, id)| id.clone())
            .collect();
        eligible.sort();
        match eligible.as_slice() {
            [survivor] => selected.push(Superseded {
                task_id: field(row, "task_id"),
                task_name: field(row, "task_name"),
                backlogged_date: field(row, "backlogged_date"),
                survivor_id: survivor.clone(),
            }),
            [] => {}
            _ => skipped.push(Skipped {
                task_id: field(row, "task_id"),
                survivor_ids: eligible,
                code: "ambiguous_survivor".into(),
            }),
        }
    }
    selected.sort_by(|a, b| a.task_id.cmp(&b.task_id));
    skipped.sort_by(|a, b| a.task_id.cmp(&b.task_id));
    (selected, skipped)
}
