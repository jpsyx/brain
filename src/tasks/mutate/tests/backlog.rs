//! The backlog parks a task without abandoning it: no schedule, hidden from
//! every active view, and reviewed monthly.

use super::{TASKS_HEADER, column, fixture, today};
use crate::tasks::mutate::backlog;

const TASK: &str = "T1,Ship it,mit,in_progress,p1,2026-08-24,2026-08-20,true,pablo,,2,,2026-08-01,Website,,2026-08-01,,2026-08-01\n";

#[test]
fn backlogging_clears_the_whole_schedule() {
    let fixture = fixture(TASK, "");

    let result = backlog::backlog_in_root(&fixture.root, &fixture.targets(), "T1", false, today())
        .expect("backlog")
        .0;

    assert_eq!(result.previous_status, "in_progress");
    let csv = fixture.tasks();
    assert_eq!(column(&csv, "T1", TASKS_HEADER, "status"), "backlog");
    assert_eq!(
        column(&csv, "T1", TASKS_HEADER, "backlogged_date"),
        "2026-08-24"
    );
    // A parked task has no schedule, and a hard deadline is meaningless
    // without a due date.
    assert_eq!(column(&csv, "T1", TASKS_HEADER, "due_date"), "");
    assert_eq!(column(&csv, "T1", TASKS_HEADER, "start_date"), "");
    assert_eq!(column(&csv, "T1", TASKS_HEADER, "hard_deadline"), "false");
    assert_eq!(column(&csv, "T1", TASKS_HEADER, "waiting_since"), "");
}

#[test]
fn a_backlogged_task_in_a_project_says_so() {
    let fixture = fixture(TASK, "");

    let result = backlog::backlog_in_root(&fixture.root, &fixture.targets(), "T1", false, today())
        .expect("backlog")
        .0;

    // Whether the *project* should follow is a judgement call for the caller.
    assert_eq!(result.project.as_deref(), Some("Website"));
}

#[test]
fn restoring_returns_it_to_the_active_list_without_a_date() {
    let parked = "T1,Ship it,,backlog,p1,,,false,pablo,,2,2026-02-01,,,,2026-08-01,,2026-08-01\n";
    let fixture = fixture(parked, "");

    backlog::backlog_in_root(&fixture.root, &fixture.targets(), "T1", true, today())
        .expect("restore");

    let csv = fixture.tasks();
    assert_eq!(column(&csv, "T1", TASKS_HEADER, "status"), "not_started");
    assert_eq!(column(&csv, "T1", TASKS_HEADER, "backlogged_date"), "");
    // The caller re-assigns a due date afterwards; restore does not invent one.
    assert_eq!(column(&csv, "T1", TASKS_HEADER, "due_date"), "");
}

#[test]
fn restoring_something_that_is_not_parked_is_refused() {
    let fixture = fixture(TASK, "");

    let error = backlog::backlog_in_root(&fixture.root, &fixture.targets(), "T1", true, today())
        .expect_err("not in the backlog");

    assert!(error.to_string().contains("not in the backlog"), "{error}");
}

#[test]
fn backlogging_something_already_parked_is_a_no_op() {
    let parked = "T1,Ship it,,backlog,p1,,,false,pablo,,2,2026-02-01,,,,2026-08-01,,2026-08-01\n";
    let fixture = fixture(parked, "");

    let result = backlog::backlog_in_root(&fixture.root, &fixture.targets(), "T1", false, today())
        .expect("already parked")
        .0;

    assert!(result.already);
    // The original parking date is not overwritten.
    assert_eq!(
        column(&fixture.tasks(), "T1", TASKS_HEADER, "backlogged_date"),
        "2026-02-01"
    );
}

#[test]
fn a_habit_cannot_be_backlogged() {
    let habit = "H1,Stretch,not_started,p2,2026-08-24,1,days,pablo,2026-08-01,,2026-08-01,\n";
    let fixture = fixture(TASK, habit);

    let error = backlog::backlog_in_root(&fixture.root, &fixture.targets(), "H1", false, today())
        .expect_err("habits recur; they cannot be parked");

    assert!(error.to_string().contains("habit"), "{error}");
}
