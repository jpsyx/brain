//! Assignment names a portable workspace member, and only a real one.

use super::{TASKS_HEADER, column, fixture, today, users_json};
use crate::tasks::mutate::assign;

const TASK: &str =
    "T1,Ship it,,not_started,p1,2026-08-24,,false,pablo,,0,,,,,2026-08-01,,2026-08-01\n";

#[test]
fn assigning_moves_the_row_to_another_member() {
    let fixture = fixture(TASK, "");
    users_json(&fixture.root, &["pablo", "kristi"]);

    let result = assign::assign_in_root(&fixture.root, &fixture.targets(), "T1", "kristi", today())
        .expect("assign")
        .0;

    assert_eq!(result.assigned_to, "kristi");
    assert_eq!(
        column(&fixture.tasks(), "T1", TASKS_HEADER, "assigned_to"),
        "kristi"
    );
    assert_eq!(
        column(&fixture.tasks(), "T1", TASKS_HEADER, "last_touched"),
        "2026-08-24"
    );
}

#[test]
fn assigning_to_a_stranger_is_refused() {
    let fixture = fixture(TASK, "");
    users_json(&fixture.root, &["pablo"]);

    let error = assign::assign_in_root(&fixture.root, &fixture.targets(), "T1", "nobody", today())
        .expect_err("not a member");

    assert!(
        error.to_string().contains("not a workspace member"),
        "{error}"
    );
    assert_eq!(
        column(&fixture.tasks(), "T1", TASKS_HEADER, "assigned_to"),
        "pablo",
        "a refused assignment must not write"
    );
}

#[test]
fn a_malformed_user_id_is_refused_before_any_lookup() {
    let fixture = fixture(TASK, "");

    let error = assign::assign_in_root(
        &fixture.root,
        &fixture.targets(),
        "T1",
        "Not An ID",
        today(),
    )
    .expect_err("invalid id");

    assert!(!error.to_string().is_empty());
}
