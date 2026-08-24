//! The pure agenda-sync decision: given the agenda markdown, the mutated id,
//! and a snapshot of the CSVs, return the synced markdown.
//!
//! One implementation serves every mutator — native completion in this binary
//! and the `/todo` mutator scripts through `brain tasks sync-agenda` — so the
//! two can never drift.

use chrono::NaiveDate;

use super::doc::Document;
use super::{CUT_HEADING, MIT_HEADING, SUGGESTED_HEADING, derive, lines};
use crate::tasks::complete::{Row, field, parse_chunk_name};

/// Which mutation the agenda is being synced for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    /// The item was completed: drop it from the actionable sections, swapping
    /// in the next chunk of a chunked task when there is one.
    Done,
    /// The item moved off today: drop it from the actionable sections.
    Defer,
    /// Nothing left today's plan; only the CSV-derived snapshots are refreshed.
    Touch,
}

impl Action {
    /// Does this action edit the actionable sections (MIT / Suggested / Cut)?
    const fn edits_plan(self) -> bool {
        matches!(self, Self::Done | Self::Defer)
    }
}

/// The CSV rows the re-derived sections are built from.
pub(crate) struct Snapshot<'a> {
    pub(crate) tasks: &'a [Row],
    pub(crate) habits: &'a [Row],
}

/// Sync `text` for `task_id`. Only the sections this function owns are
/// rewritten; the title, `**Load:**`, `**Bottom line:**`, and any unrelated
/// section come back byte-for-byte.
pub(crate) fn sync_markdown(
    text: &str,
    task_id: &str,
    action: Action,
    snapshot: &Snapshot<'_>,
    today: NaiveDate,
) -> String {
    let mut doc = Document::parse(text);
    if action.edits_plan() {
        let next_chunk = if action == Action::Done {
            next_chunk_row(snapshot.tasks, task_id)
        } else {
            None
        };
        if let Some(index) = doc.find(MIT_HEADING) {
            let body = &doc.sections[index].body;
            doc.sections[index].body = next_chunk.map_or_else(
                || lines::drop_lines_with_id(body, task_id, false),
                |next| lines::swap_chunk_in_mit(body, task_id, next),
            );
        }
        if let Some(index) = doc.find(SUGGESTED_HEADING) {
            let body = &doc.sections[index].body;
            doc.sections[index].body = next_chunk.map_or_else(
                || lines::drop_lines_with_id(body, task_id, true),
                |next| lines::swap_chunk_in_suggested(body, task_id, next),
            );
        }
        if let Some(index) = doc.find(CUT_HEADING) {
            let body = &doc.sections[index].body;
            doc.sections[index].body = lines::drop_lines_with_id(body, task_id, true);
        }
    }

    doc.replace_or_set(
        super::HABITS_HEADING,
        derive::today_habits(snapshot.habits, today),
    );
    doc.replace_or_set(
        super::COMPLETED_HEADING,
        derive::completed_today_section(snapshot.tasks, snapshot.habits, today),
    );
    doc.render()
}

/// The next unfinished chunk in `task_id`'s family, or `None` when the id isn't
/// a chunked task, is the last chunk, or the next chunk is already done.
///
/// Read from the post-mutation snapshot, so the just-completed row is already
/// `status=done` here.
fn next_chunk_row<'a>(tasks: &'a [Row], task_id: &str) -> Option<&'a Row> {
    if !task_id.starts_with('T') {
        return None;
    }
    let completed = tasks
        .iter()
        .find(|row| field(row, "task_id").trim() == task_id)?;
    let (base, index, total) = parse_chunk_name(&field(completed, "task_name"))?;
    if index >= total {
        return None;
    }
    let target = format!("{base} ({}/{total})", index + 1);
    let next = tasks
        .iter()
        .find(|row| field(row, "task_name").trim() == target)?;
    if field(next, "status").trim() == "done" {
        return None;
    }
    Some(next)
}
