//! Touch is the smallest mutation there is: it moves one column.

use super::{TASKS_HEADER, column, fixture, today};
use crate::tasks::mutate::touch;

const TASK: &str =
    "T1,Ship it,,not_started,p1,2026-08-24,,false,pablo,,0,,,,,2026-08-01,,2026-06-01\n";

#[test]
fn touching_a_task_only_moves_last_touched() {
    let fixture = fixture(TASK, "");

    let result = touch::touch_in_root(&fixture.root, &fixture.targets(), "T1", today())
        .expect("touch")
        .0;

    assert_eq!(result.previous, "2026-06-01");
    assert_eq!(
        column(&fixture.tasks(), "T1", TASKS_HEADER, "last_touched"),
        "2026-08-24"
    );
    // Nothing else moved: this is the "yes, I still care" acknowledgement.
    assert_eq!(
        column(&fixture.tasks(), "T1", TASKS_HEADER, "status"),
        "not_started"
    );
    assert_eq!(
        column(&fixture.tasks(), "T1", TASKS_HEADER, "due_date"),
        "2026-08-24"
    );
    assert_eq!(
        column(&fixture.tasks(), "T1", TASKS_HEADER, "defer_count"),
        "0"
    );
}

#[test]
fn a_never_touched_row_reports_that() {
    let never = "T1,Ship it,,not_started,p1,2026-08-24,,false,pablo,,0,,,,,2026-08-01,,\n";
    let fixture = fixture(never, "");

    let result = touch::touch_in_root(&fixture.root, &fixture.targets(), "T1", today())
        .expect("touch")
        .0;

    assert_eq!(result.previous, "(never)");
}
