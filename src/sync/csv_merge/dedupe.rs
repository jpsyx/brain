//! Cross-row de-duplication for habit occurrences that raced across two
//! machines before syncing.
//!
//! The id-keyed merge in [`super::merge`] can only see identity via
//! `task_uuid`. That's correct for edits to an *existing* row, but a habit's
//! next occurrence is a brand-new row (`TaskUuid::new()` — see
//! `tasks::complete::complete_ops::spawn_next_occurrence` and
//! `tasks::triage_habits::reconcile::reconcile_enabled`). If the same
//! occurrence gets spawned independently on two machines before they sync,
//! each spawn allocates its own fresh, distinct `task_uuid` (and, since the
//! two machines' `.habits_next_id` counters are independent until synced,
//! almost always a distinct `task_id` too). Neither side's table contains a
//! duplicate on its own — the duplication only exists in the union — so
//! nothing in the id-keyed merge ever sees it as a same-row conflict; both
//! rows survive as unrelated "added" rows.
//!
//! This pass runs after the id-keyed union and collapses any rows that
//! share a `(task_name, due_date)` back down to one, folding their fields
//! through the same completion-wins / last-touched-wins rules as any other
//! merge conflict (see [`super::merge::field_merge`]) — so a row marked
//! `done` always wins over one that isn't, regardless of which side is
//! "ours" vs. "theirs" or which has the newer `last_touched`. It is safe to
//! run unconditionally: a table with no duplicates, or an already-deduped
//! table, is left untouched. It never fires on `tasks.csv`, whose schema has
//! no `recur_interval` column.

use std::collections::BTreeMap;

use super::Table;
use super::merge::{Cols, field_merge};

/// Collapse habit rows that share `(task_name, due_date)` into one. Returns
/// the count of rows removed and any soft-conflict notes worth surfacing in
/// the sync journal.
pub(super) fn dedupe_habit_occurrences(table: &mut Table) -> (usize, Vec<String>) {
    if !table.is_uuid_keyed() || table.column("recur_interval").is_none() {
        // Not a current-schema, habit-shaped table (e.g. tasks.csv) —
        // nothing to dedupe.
        return (0, Vec::new());
    }
    let Some(name_index) = table.column("task_name") else {
        return (0, Vec::new());
    };
    let Some(due_index) = table.column("due_date") else {
        return (0, Vec::new());
    };

    let mut groups: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for (uuid, row) in &table.rows {
        let key = (
            row.get(name_index).cloned().unwrap_or_default(),
            row.get(due_index).cloned().unwrap_or_default(),
        );
        groups.entry(key).or_default().push(uuid.clone());
    }

    let columns = Cols::from_header(&table.header);
    let header = table.header.clone();
    let mut removed = 0usize;
    let mut notes = Vec::new();

    for ((name, due_date), mut uuids) in groups {
        if uuids.len() < 2 {
            continue;
        }
        // Sort so the surviving identity is a pure function of the group's
        // content, never of merge side or iteration order — required for
        // convergence (swapping `ours`/`theirs` must stay byte-identical).
        uuids.sort();
        let survivor = uuids[0].clone();
        let id = format!("habit occurrence \"{name}\" due {due_date}");
        let mut folded = table.rows.get(&survivor).cloned().unwrap_or_default();
        for loser in &uuids[1..] {
            let Some(loser_row) = table.rows.get(loser).cloned() else {
                continue;
            };
            let (row, folded_notes) =
                field_merge(None, &folded, &loser_row, &columns, &header, &id);
            folded = row;
            notes.extend(folded_notes);
        }
        for loser in &uuids[1..] {
            table.rows.remove(loser);
            removed += 1;
        }
        table.rows.insert(survivor.clone(), folded);
        notes.push(format!(
            "{id}: collapsed {} duplicate row(s) spawned independently across machines (kept {survivor})",
            uuids.len() - 1
        ));
    }

    (removed, notes)
}

#[cfg(test)]
mod tests;
