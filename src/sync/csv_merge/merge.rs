//! UUID/legacy three-way row merge with name-aligned fields.

use std::collections::{BTreeMap, BTreeSet};

use super::Table;
use super::reconcile::{maximum_display_number, reconcile};
use super::relationships::{emit_final_references, resolve_side_references};

/// Soft merge outcomes worth surfacing in the sync journal.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub added: usize,
    pub deleted: usize,
    pub merged: usize,
    pub soft_conflicts: Vec<String>,
}

struct Cols {
    status: Option<usize>,
    completed: Option<usize>,
    last_touched: Option<usize>,
}

impl Cols {
    fn from_header(header: &[String]) -> Self {
        let find = |name: &str| header.iter().position(|column| column == name);
        Self {
            status: find("status"),
            completed: find("completed_date"),
            last_touched: find("last_touched"),
        }
    }
}

fn choose_header(base: &Table, ours: &Table, theirs: &Table) -> Vec<String> {
    let primary = if base.header.is_empty() {
        match (ours.header.is_empty(), theirs.header.is_empty()) {
            (false, true) => Some(&ours.header),
            (true, false) => Some(&theirs.header),
            _ => Some(std::cmp::min(&ours.header, &theirs.header)),
        }
    } else {
        Some(&base.header)
    };
    let mut names = BTreeSet::new();
    for column in ours
        .header
        .iter()
        .chain(theirs.header.iter())
        .chain(base.header.iter())
    {
        names.insert(column.clone());
    }
    let mut header = primary
        .into_iter()
        .flatten()
        .filter(|column| names.remove(*column))
        .cloned()
        .collect::<Vec<_>>();
    header.extend(names);
    if let Some(index) = header.iter().position(|column| column == "task_uuid") {
        header.swap(0, index);
    }
    header
}

fn resolve_conflict(
    ours: &[String],
    theirs: &[String],
    columns: &Cols,
    header: &[String],
    index: usize,
    id: &str,
    notes: &mut Vec<String>,
) -> String {
    let (our_value, their_value) = (&ours[index], &theirs[index]);
    columns.last_touched.map_or_else(
        || {
            let column = header.get(index).map_or("", String::as_str);
            notes.push(format!(
                "task identity {id}: conflicting {column} values; kept the greater"
            ));
            our_value.max(their_value).clone()
        },
        |last_touched| {
            if (ours[last_touched].as_str(), our_value.as_str())
                >= (theirs[last_touched].as_str(), their_value.as_str())
            {
                our_value.clone()
            } else {
                their_value.clone()
            }
        },
    )
}

fn field_merge(
    base: Option<&[String]>,
    ours: &[String],
    theirs: &[String],
    columns: &Cols,
    header: &[String],
    id: &str,
) -> (Vec<String>, Vec<String>) {
    let mut output = vec![String::new(); header.len()];
    let mut resolved = vec![false; header.len()];
    let mut notes = Vec::new();
    let ours_done = columns.status.is_some_and(|index| ours[index] == "done");
    let theirs_done = columns.status.is_some_and(|index| theirs[index] == "done");
    if ours_done != theirs_done {
        let done = if ours_done { ours } else { theirs };
        for index in [columns.status, columns.completed].into_iter().flatten() {
            output[index].clone_from(&done[index]);
            resolved[index] = true;
        }
    }
    for index in 0..header.len() {
        if resolved[index] {
            continue;
        }
        let (our_value, their_value) = (&ours[index], &theirs[index]);
        output[index] = match base {
            Some(base) if our_value == &base[index] => their_value.clone(),
            Some(base) if their_value == &base[index] => our_value.clone(),
            _ if our_value == their_value => our_value.clone(),
            _ => resolve_conflict(ours, theirs, columns, header, index, id, &mut notes),
        };
    }
    (output, notes)
}

/// Three-way merge by immutable `task_uuid` when present, retaining legacy
/// `task_id` behavior for workspaces whose coordinated migration is inactive.
#[must_use]
pub fn merge(base: &Table, ours: &Table, theirs: &Table) -> (Table, Report) {
    let allocation_floor = maximum_display_number(&[base, ours, theirs]);
    let base = resolve_side_references(base);
    let ours = resolve_side_references(ours);
    let theirs = resolve_side_references(theirs);
    let header = choose_header(&base, &ours, &theirs);
    let columns = Cols::from_header(&header);
    let mut rows = BTreeMap::new();
    let mut report = Report::default();
    let ids = base
        .rows
        .keys()
        .chain(ours.rows.keys())
        .chain(theirs.rows.keys())
        .map(String::as_str)
        .collect::<BTreeSet<_>>();

    for id in ids {
        let base_row = base.rows.get(id).map(|row| base.aligned_row(row, &header));
        let our_row = ours.rows.get(id).map(|row| ours.aligned_row(row, &header));
        let their_row = theirs
            .rows
            .get(id)
            .map(|row| theirs.aligned_row(row, &header));
        match (base_row, our_row, their_row) {
            (None, Some(row), None) | (None, None, Some(row)) => {
                rows.insert(id.to_owned(), row);
                report.added += 1;
            }
            (None, Some(ours), Some(theirs)) => {
                let (row, notes) = field_merge(None, &ours, &theirs, &columns, &header, id);
                rows.insert(id.to_owned(), row);
                report.added += 1;
                report.soft_conflicts.extend(notes);
            }
            (Some(_), None, None) => report.deleted += 1,
            (Some(base), Some(side), None) | (Some(base), None, Some(side)) => {
                if side == base {
                    report.deleted += 1;
                } else {
                    rows.insert(id.to_owned(), side);
                    report.soft_conflicts.push(format!(
                        "task identity {id}: deleted on one side but edited on the other; kept the edit"
                    ));
                }
            }
            (Some(base), Some(ours), Some(theirs)) => match (ours != base, theirs != base) {
                (false, false) => {
                    rows.insert(id.to_owned(), base);
                }
                (true, false) => {
                    rows.insert(id.to_owned(), ours);
                }
                (false, true) => {
                    rows.insert(id.to_owned(), theirs);
                }
                (true, true) => {
                    let (row, notes) =
                        field_merge(Some(&base), &ours, &theirs, &columns, &header, id);
                    rows.insert(id.to_owned(), row);
                    report.merged += 1;
                    report.soft_conflicts.extend(notes);
                }
            },
            (None, None, None) => {}
        }
    }
    let mut merged = Table { header, rows };
    reconcile(&mut merged, allocation_floor);
    (emit_final_references(&merged), report)
}
