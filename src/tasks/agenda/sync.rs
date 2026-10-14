//! The pure agenda-sync decision: given the agenda markdown, the mutated id,
//! and a snapshot of the CSVs, return the synced markdown.
//!
//! One implementation serves every mutator — native completion in this binary
//! and the `/todo` mutator scripts through `brain tasks sync-agenda` — so the
//! two can never drift.

use chrono::NaiveDate;

use super::doc::Document;
use super::{CUT_HEADING, MIT_HEADING, SUGGESTED_HEADING, derive, lines};
use crate::tasks::complete::{Row, field};

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
) -> Result<String, String> {
    let mut doc = Document::parse(text);
    if action.edits_plan() {
        let next_chunk = if action == Action::Done {
            next_chunk_row(snapshot.tasks, task_id)?
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
    Ok(doc.render())
}

/// Resolve from the post-mutation snapshot, where the anchor is already done.
fn next_chunk_row<'a>(tasks: &'a [Row], task_id: &str) -> Result<Option<&'a Row>, String> {
    if !task_id.starts_with('T') {
        return Ok(None);
    }
    let Some(completed) = tasks
        .iter()
        .find(|row| field(row, "task_id").trim() == task_id)
    else {
        return Ok(None);
    };
    crate::tasks::chunk_family::successor(completed, tasks)
}

#[cfg(test)]
mod chunk_handoff_tests {
    use super::*;

    #[test]
    fn gap_handoff_keeps_time_slot_and_does_not_duplicate_existing_entries() {
        let tasks = [
            [
                ("task_id", "T1"),
                ("task_name", "Read spec (1/4)"),
                ("status", "done"),
            ],
            [
                ("task_id", "T3"),
                ("task_name", "Read spec (3/4)"),
                ("status", "waiting"),
            ],
        ]
        .map(|fields| {
            fields
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect()
        });
        let snapshot = Snapshot {
            tasks: &tasks,
            habits: &[],
        };
        let today = NaiveDate::from_ymd_opt(2026, 10, 14).unwrap();
        for present in [false, true] {
            let mut text = "## ❗ MIT

- [ ] ❗ **T1** Read spec (1/4)

## Suggested order

1. [ ] 09:00 | **T1** Read spec (1/4)
"
            .to_owned();
            if present {
                text = text.replace(
                    "## Suggested",
                    "- [ ] ❗ **T3** Read spec (3/4)

## Suggested",
                );
                text.push_str(
                    "2. [ ] 10:00 | **T3** Read spec (3/4)
",
                );
            }
            let out = sync_markdown(&text, "T1", Action::Done, &snapshot, today).unwrap();
            assert_eq!(out.matches("**T3**").count(), 2, "{out}");
            assert!(
                out.contains(if present {
                    "1. [ ] 10:00 | **T3**"
                } else {
                    "1. [ ] 09:00 | **T3**"
                }),
                "{out}"
            );
        }
    }

    #[test]
    fn ambiguity_is_reported_and_leaves_agenda_bytes_unchanged() {
        use crate::tasks::agenda::{Outcome, Targets, sync_targets};
        let root = tempfile::tempdir().unwrap();
        let tasks_dir = root.path().join("tasks");
        std::fs::create_dir(&tasks_dir).unwrap();
        std::fs::write(tasks_dir.join("tasks.csv"), "task_id,task_name,status,project\nT1,Read spec (1/4),done,alpha\nT3,Read spec (3/4),not_started,alpha\nT9,Read spec (3/4),done,alpha\n").unwrap();
        let markdown = root.path().join("agenda.md");
        let text = "## ❗ MIT\n\n- [ ] ❗ **T1** Read spec (1/4)\n";
        std::fs::write(&markdown, text).unwrap();
        let targets = Targets {
            markdown: markdown.clone(),
            pdf: root.path().join("agenda.pdf"),
            renderer: None,
            tasks_dir: tasks_dir.clone(),
        };
        let tasks = crate::tasks::complete::read_csv(&tasks_dir.join("tasks.csv")).unwrap();
        let date = NaiveDate::from_ymd_opt(2026, 10, 14).unwrap();
        assert_eq!(
            sync_markdown(
                text,
                "T1",
                Action::Done,
                &Snapshot {
                    tasks: &tasks.rows,
                    habits: &[]
                },
                date
            )
            .unwrap_err(),
            "ambiguous_chunk_family"
        );
        assert_eq!(
            sync_targets(&targets, "T1", Action::Done, date),
            Outcome::NoAgenda
        );
        assert_eq!(std::fs::read_to_string(markdown).unwrap(), text);
    }
}
