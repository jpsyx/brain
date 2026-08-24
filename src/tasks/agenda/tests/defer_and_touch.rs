//! Deferring drops the item without promoting a sibling; touching only
//! refreshes the CSV-derived snapshots.

use super::{FULL_AGENDA, row, today};
use crate::tasks::agenda::{Action, Snapshot, sync_markdown};

#[test]
fn defer_drops_the_task_without_promoting_the_next_chunk() {
    let agenda = "\
## Suggested order

1. [ ] 09:00 | **T10** Read the spec (1/3) (45m)
";
    let tasks = [
        row(&[
            ("task_id", "T10"),
            ("task_name", "Read the spec (1/3)"),
            ("status", "not_started"),
        ]),
        row(&[
            ("task_id", "T11"),
            ("task_name", "Read the spec (2/3)"),
            ("status", "not_started"),
        ]),
    ];

    let out = sync_markdown(
        agenda,
        "T10",
        Action::Defer,
        &Snapshot {
            tasks: &tasks,
            habits: &[],
        },
        today(),
    );

    assert!(!out.contains("**T10**"), "{out}");
    assert!(!out.contains("**T11**"), "{out}");
}

#[test]
fn touch_leaves_the_plan_alone() {
    let tasks = [row(&[
        ("task_id", "T535"),
        ("task_name", "Fix the sync"),
        ("status", "not_started"),
    ])];

    let out = sync_markdown(
        FULL_AGENDA,
        "T535",
        Action::Touch,
        &Snapshot {
            tasks: &tasks,
            habits: &[],
        },
        today(),
    );

    // Nothing was completed and no habit qualifies, so a touch is a no-op.
    assert_eq!(out, FULL_AGENDA);
}
