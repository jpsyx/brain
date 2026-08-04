//! Deterministic mutable display-ID collision reconciliation.

use std::collections::BTreeMap;

use super::Table;

#[must_use]
pub(crate) fn maximum_display_number(tables: &[&Table]) -> u32 {
    tables
        .iter()
        .filter_map(|table| table.column("task_id").map(|index| (*table, index)))
        .flat_map(|(table, index)| table.rows.values().filter_map(move |row| row.get(index)))
        .filter_map(|display| display_number(display))
        .max()
        .unwrap_or(0)
}

pub(crate) fn reconcile(table: &mut Table, allocation_floor: u32) {
    if !table.is_uuid_keyed() {
        return;
    }
    let Some(display_index) = table.column("task_id") else {
        return;
    };
    let mut claims = BTreeMap::<String, Vec<String>>::new();
    for (uuid, row) in &table.rows {
        if let Some(display) = row.get(display_index) {
            claims
                .entry(display.clone())
                .or_default()
                .push(uuid.clone());
        }
    }
    let mut losers = Vec::new();
    for (display, mut uuids) in claims {
        if uuids.len() < 2 {
            continue;
        }
        uuids.sort();
        losers.extend(
            uuids
                .into_iter()
                .skip(1)
                .map(|uuid| (display.clone(), uuid)),
        );
    }
    losers.sort_by(|left, right| left.1.cmp(&right.1));
    let mut next = allocation_floor;
    for (prior_display, uuid) in losers {
        next = next.saturating_add(1);
        let prefix = prior_display
            .chars()
            .next()
            .filter(char::is_ascii_alphabetic)
            .unwrap_or('T');
        if let Some(row) = table.rows.get_mut(&uuid)
            && let Some(display) = row.get_mut(display_index)
        {
            *display = format!("{prefix}{next}");
        }
    }
}

#[must_use]
pub(crate) fn display_number(display: &str) -> Option<u32> {
    display
        .trim()
        .strip_prefix(['T', 'H'])
        .and_then(|number| number.parse().ok())
}
