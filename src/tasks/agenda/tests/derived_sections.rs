//! "Today's habits" and "Completed today" are rebuilt from the CSVs on every
//! sync, so state changed outside this process still shows up.

use super::{row, today};
use crate::tasks::agenda::{Action, Snapshot, sync_markdown};

fn synced(
    agenda: &str,
    tasks: &[crate::tasks::complete::Row],
    habits: &[crate::tasks::complete::Row],
) -> String {
    sync_markdown(
        agenda,
        "T999",
        Action::Touch,
        &Snapshot { tasks, habits },
        today(),
    )
}

#[test]
fn habits_are_listed_pending_first_ordered_by_ideal_time() {
    let habits = [
        row(&[
            ("task_id", "H2"),
            ("task_name", "Stretch"),
            ("status", "not_started"),
            ("ideal_time", "08:00"),
        ]),
        row(&[
            ("task_id", "H1"),
            ("task_name", "Walk the dog"),
            ("status", "not_started"),
            ("ideal_time", "07:00"),
        ]),
        row(&[
            ("task_id", "H3"),
            ("task_name", "Journal"),
            ("status", "done"),
            ("completed_date", "2026-08-24"),
        ]),
    ];

    let out = synced("# Agenda\n", &[], &habits);

    assert!(
        out.contains("| ◻ **H1** Walk the dog | ◻ **H2** Stretch |"),
        "{out}"
    );
    assert!(out.contains("| ✅ **H3** Journal |  |"), "{out}");
}

#[test]
fn a_habit_due_later_is_not_on_today() {
    let habits = [row(&[
        ("task_id", "H9"),
        ("task_name", "Quarterly review"),
        ("status", "not_started"),
        ("due_date", "2026-09-01"),
    ])];

    let out = synced("# Agenda\n", &[], &habits);

    assert!(!out.contains("## 🔁"), "{out}");
}

#[test]
fn a_stale_habits_section_is_removed_when_nothing_qualifies() {
    let agenda = "\
# Agenda

## 🔁 Today's habits

|  |  |
|---|---|
| ◻ **H1** Walk the dog |  |

## Cut order

1. **T1** Ship it
";
    let out = synced(agenda, &[], &[]);

    assert!(!out.contains("## 🔁"), "{out}");
    assert!(!out.contains("**H1**"), "{out}");
    assert!(out.contains("1. **T1** Ship it"), "{out}");
}

#[test]
fn completed_today_lists_habits_before_tasks_and_ignores_other_days() {
    let tasks = [
        row(&[
            ("task_id", "T1"),
            ("task_name", "Ship it"),
            ("status", "done"),
            ("completed_date", "2026-08-24"),
        ]),
        row(&[
            ("task_id", "T2"),
            ("task_name", "Yesterday's thing"),
            ("status", "done"),
            ("completed_date", "2026-08-23"),
        ]),
    ];
    let habits = [row(&[
        ("task_id", "H1"),
        ("task_name", "Journal"),
        ("status", "done"),
        ("completed_date", "2026-08-24"),
    ])];

    let out = synced("# Agenda\n", &tasks, &habits);

    assert!(
        out.contains("| ✅ **H1** Journal | ✅ **T1** Ship it |"),
        "{out}"
    );
    assert!(!out.contains("Yesterday's thing"), "{out}");
}
