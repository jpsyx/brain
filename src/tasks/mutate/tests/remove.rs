//! Removal is protected: it can delete a task by accident far more easily
//! than a habit chain should ever be deleted on purpose.

use super::{TASKS_HEADER, column, fixture, today};
use crate::tasks::mutate::remove;

const TASK: &str =
    "T1,Ship it,,not_started,p1,2026-08-24,,false,pablo,,0,,,,,2026-08-01,,2026-08-01\n";
const HABIT: &str = "H1,Stretch,not_started,p2,2026-08-24,1,days,pablo,2026-08-01,,2026-08-01,\n";

#[test]
fn removing_a_task_drops_its_row() {
    let fixture = fixture(TASK, HABIT);

    let result = remove::remove_in_root(&fixture.root, &fixture.targets(), "T1", false, today())
        .expect("remove")
        .0;

    assert_eq!(result.task_id, "T1");
    assert!(!result.was_habit);
    assert_eq!(fixture.tasks().lines().count(), 1, "{}", fixture.tasks());
}

#[test]
fn removing_a_habit_needs_an_explicit_opt_in() {
    let fixture = fixture(TASK, HABIT);

    let error = remove::remove_in_root(&fixture.root, &fixture.targets(), "H1", false, today())
        .expect_err("habit removal must be refused");

    // Deleting a habit row destroys every future occurrence, so a task-cleanup
    // pass must be structurally unable to reach it.
    assert!(error.to_string().contains("--habit"), "{error}");
    assert_eq!(fixture.habits().lines().count(), 2, "{}", fixture.habits());
}

#[test]
fn the_habit_opt_in_is_refused_for_a_task() {
    let fixture = fixture(TASK, HABIT);

    let error = remove::remove_in_root(&fixture.root, &fixture.targets(), "T1", true, today())
        .expect_err("--habit on a task must be refused");

    assert!(error.to_string().contains("is a task"), "{error}");
}

#[test]
fn a_habit_removed_with_the_opt_in_is_gone() {
    let fixture = fixture(TASK, HABIT);

    let result = remove::remove_in_root(&fixture.root, &fixture.targets(), "H1", true, today())
        .expect("remove habit")
        .0;

    assert!(result.was_habit);
    assert_eq!(fixture.habits().lines().count(), 1, "{}", fixture.habits());
}

#[test]
fn a_managed_triage_row_is_never_removable() {
    let managed = "H9,Morning Triage,not_started,p1,2026-08-24,1,days,pablo,2026-08-01,,2026-08-01,brain.triage.daily\n";
    let fixture = fixture(TASK, managed);

    let error = remove::remove_in_root(&fixture.root, &fixture.targets(), "H9", true, today())
        .expect_err("managed rows are protected");

    assert!(
        error.to_string().to_lowercase().contains("managed"),
        "{error}"
    );
}

#[test]
fn removal_leaves_the_rest_of_the_file_intact() {
    let two = format!(
        "{TASK}T2,Keep me,,not_started,p2,2026-08-25,,false,pablo,,0,,,,,2026-08-01,,2026-08-01\n"
    );
    let fixture = fixture(&two, HABIT);

    remove::remove_in_root(&fixture.root, &fixture.targets(), "T1", false, today())
        .expect("remove");

    assert_eq!(
        column(&fixture.tasks(), "T2", TASKS_HEADER, "task_name"),
        "Keep me"
    );
}
