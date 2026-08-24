//! Chunk-family ordering.
//!
//! A chunked task is `<base> (<i>/<N>)`. The family only makes sense in order,
//! so pushing one chunk out has to push any later sibling that would otherwise
//! land before it. Never the reverse: a chunk already scheduled later is left
//! where the user put it.

use chrono::NaiveDate;

use crate::tasks::complete::{Row, field, parse_chunk_name, parse_date, touch_row};

/// One chunk the cascade moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Cascaded {
    pub(crate) task_id: String,
    pub(crate) task_name: String,
    pub(crate) due_date: String,
}

/// Every chunk in `anchor`'s family with a strictly greater index, in index
/// order. Empty when `anchor` is not a chunk or has no later siblings.
fn later_chunks(rows: &[Row], anchor: &Row) -> Vec<usize> {
    let Some((base, index, total)) = parse_chunk_name(&field(anchor, "task_name")) else {
        return Vec::new();
    };
    let mut later: Vec<(u32, usize)> = rows
        .iter()
        .enumerate()
        .filter_map(|(position, row)| {
            let (row_base, row_index, row_total) = parse_chunk_name(&field(row, "task_name"))?;
            (row_base == base && row_total == total && row_index > index)
                .then_some((row_index, position))
        })
        .collect();
    later.sort_unstable();
    later.into_iter().map(|(_, position)| position).collect()
}

/// Push later chunks forward so the family order stays valid.
///
/// `defer_count` is deliberately **not** propagated: a cascade is the anchor's
/// slip, not the cascaded chunk's.
pub(crate) fn cascade_forward(rows: &mut [Row], anchor_index: usize, today: &str) -> Vec<Cascaded> {
    let Some(anchor) = rows.get(anchor_index) else {
        return Vec::new();
    };
    let Some(mut floor) = parse_date(&field(anchor, "due_date")) else {
        return Vec::new();
    };
    let positions = later_chunks(rows, anchor);
    let mut moved = Vec::new();
    for position in positions {
        let Some(row) = rows.get_mut(position) else {
            continue;
        };
        let current = parse_date(&field(row, "due_date"));
        if current.is_some_and(|date| date >= floor) {
            floor = current.unwrap_or(floor);
            continue;
        }
        row.insert("due_date".to_owned(), floor.to_string());
        touch_row(row, today);
        moved.push(Cascaded {
            task_id: field(row, "task_id"),
            task_name: field(row, "task_name"),
            due_date: floor.to_string(),
        });
    }
    moved
}

/// Format the cascade for a report line.
#[allow(dead_code)]
pub(crate) fn describe(cascaded: &[Cascaded]) -> Option<String> {
    (!cascaded.is_empty()).then(|| {
        cascaded
            .iter()
            .map(|chunk| format!("{} {} → {}", chunk.task_id, chunk.task_name, chunk.due_date))
            .collect::<Vec<_>>()
            .join(", ")
    })
}

/// The date a `+Nd` push lands on, anchored to the row's own due date when it
/// has one and to today when it does not.
pub(crate) fn shift(due: &str, days: u32, today: NaiveDate) -> NaiveDate {
    parse_date(due).unwrap_or(today) + chrono::Days::new(u64::from(days))
}
