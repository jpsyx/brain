//! Completing an item: it leaves the actionable sections and shows up in the
//! re-derived snapshot, and nothing else in the file moves.

use super::{FULL_AGENDA, row, today};
use crate::tasks::agenda::{Action, Snapshot, sync_markdown};

#[test]
fn done_drops_the_completed_task_and_preserves_every_other_section() {
    let tasks = [
        row(&[
            ("task_id", "T535"),
            ("task_name", "Fix the sync"),
            ("status", "done"),
            ("completed_date", "2026-08-24"),
        ]),
        row(&[
            ("task_id", "T536"),
            ("task_name", "Write the docs"),
            ("status", "not_started"),
        ]),
    ];

    let out = sync_markdown(
        FULL_AGENDA,
        "T535",
        Action::Done,
        &Snapshot {
            tasks: &tasks,
            habits: &[],
        },
        today(),
    );

    assert_eq!(
        out,
        "\
# Monday 2026-08-24

**Load:** 4 tasks, 3 habits
**Bottom line:** ship the sync.

## ❗ Most important

- [ ] ❗ **T536** Write the docs (30m)

## Suggested order

1. [ ] 10:00 | **T536** Write the docs (30m)

## Cut order

1. **T536** Write the docs

## Notes to self

Core has never heard of this section.

## ✅ Completed today

|  |  |
|---|---|
| ✅ **T535** Fix the sync |  |

"
    );
}

#[test]
fn done_hands_the_vacated_slots_to_the_next_chunk() {
    let agenda = "\
## ❗ Most important

- [ ] ❗ **T10** Read the spec (1/3) (45m)

## Suggested order

1. [ ] 09:00 | **T10** Read the spec (1/3) (45m)
2. [ ] 10:00 | **T20** Reply to Sam (10m)
";
    let tasks = [
        row(&[
            ("task_id", "T10"),
            ("task_name", "Read the spec (1/3)"),
            ("status", "done"),
            ("completed_date", "2026-08-24"),
        ]),
        row(&[
            ("task_id", "T11"),
            ("task_name", "Read the spec (2/3)"),
            ("status", "not_started"),
            ("estimated_duration", "45"),
        ]),
    ];

    let out = sync_markdown(
        agenda,
        "T10",
        Action::Done,
        &Snapshot {
            tasks: &tasks,
            habits: &[],
        },
        today(),
    );

    // The next chunk inherits the callout and the time slot, so exactly one
    // actionable chunk stays visible.
    assert!(
        out.contains("- [ ] ❗ **T11** Read the spec (2/3) (45m)"),
        "{out}"
    );
    assert!(
        out.contains("1. [ ] 09:00 | **T11** Read the spec (2/3) (45m)"),
        "{out}"
    );
    assert!(
        out.contains("2. [ ] 10:00 | **T20** Reply to Sam (10m)"),
        "{out}"
    );
}

#[test]
fn an_already_listed_next_chunk_is_not_duplicated() {
    let agenda = "\
## Suggested order

1. [ ] 09:00 | **T10** Read the spec (1/3) (45m)
2. [ ] 10:00 | **T11** Read the spec (2/3) (45m)
";
    let tasks = [
        row(&[
            ("task_id", "T10"),
            ("task_name", "Read the spec (1/3)"),
            ("status", "done"),
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
        Action::Done,
        &Snapshot {
            tasks: &tasks,
            habits: &[],
        },
        today(),
    );

    assert_eq!(out.matches("**T11**").count(), 1, "{out}");
    assert!(out.contains("1. [ ] 10:00 | **T11**"), "{out}");
}

#[test]
fn a_completed_habit_leaves_the_plan_and_joins_both_snapshots() {
    let agenda = "\
## ❗ Most important

- [ ] ❗ **H304** Walk the dog

## 🔁 Today's habits

|  |  |
|---|---|
| ◻ **H304** Walk the dog | ◻ **H311** Stretch |
";
    let habits = [
        row(&[
            ("task_id", "H304"),
            ("task_name", "Walk the dog"),
            ("status", "done"),
            ("completed_date", "2026-08-24"),
            ("ideal_time", "07:00"),
        ]),
        row(&[
            ("task_id", "H311"),
            ("task_name", "Stretch"),
            ("status", "not_started"),
            ("ideal_time", "08:00"),
        ]),
    ];

    let out = sync_markdown(
        agenda,
        "H304",
        Action::Done,
        &Snapshot {
            tasks: &[],
            habits: &habits,
        },
        today(),
    );

    assert!(!out.contains("- [ ] ❗ **H304**"), "{out}");
    assert!(
        out.contains("| ◻ **H311** Stretch | ✅ **H304** Walk the dog |"),
        "{out}"
    );
    assert!(out.contains("## ✅ Completed today"), "{out}");
    assert!(out.contains("| ✅ **H304** Walk the dog |  |"), "{out}");
}
