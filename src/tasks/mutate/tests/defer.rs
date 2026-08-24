//! Deferring is the one mutation that carries a penalty, and the penalty is
//! the whole point: `defer_count` is the "are we avoiding this?" signal.

use super::{TASKS_HEADER, column, fixture, today};
use crate::tasks::mutate::defer::{self, When};

const HABIT: &str = "";

fn task(id: &str, extra: &str) -> String {
    format!(
        "{id},Ship it,mit,not_started,p0,2026-08-24,,false,pablo,{extra},2026-08-01,,2026-08-01\n"
    )
}

/// `blocked_by,defer_count,backlogged_date,waiting_since,project,linear_issue`
const PLAIN: &str = ",0,,,,";

#[test]
fn a_relative_push_moves_the_due_date_and_counts_it() {
    let fixture = fixture(&task("T1", PLAIN), HABIT);

    let result = defer::defer_in_root(
        &fixture.root,
        &fixture.targets(),
        "T1",
        When::Days(7),
        false,
        today(),
    )
    .expect("defer")
    .0;

    assert_eq!(result.new_due, "2026-08-31");
    assert_eq!(
        column(&fixture.tasks(), "T1", TASKS_HEADER, "defer_count"),
        "1"
    );
}

#[test]
fn an_absolute_date_is_used_verbatim() {
    let fixture = fixture(&task("T1", PLAIN), HABIT);

    let result = defer::defer_in_root(
        &fixture.root,
        &fixture.targets(),
        "T1",
        When::On(NaiveDate::from_ymd_opt(2026, 9, 15).expect("date")),
        false,
        today(),
    )
    .expect("defer")
    .0;

    assert_eq!(result.new_due, "2026-09-15");
}

#[test]
fn deferring_demotes_a_p0_and_sheds_the_mit_tag() {
    let fixture = fixture(&task("T1", PLAIN), HABIT);

    // If it can wait, it is no longer urgent *and* critical.
    defer::defer_in_root(
        &fixture.root,
        &fixture.targets(),
        "T1",
        When::Days(1),
        false,
        today(),
    )
    .expect("defer");

    assert_eq!(
        column(&fixture.tasks(), "T1", TASKS_HEADER, "priority"),
        "p1"
    );
    assert_eq!(
        column(&fixture.tasks(), "T1", TASKS_HEADER, "task_type"),
        ""
    );
}

#[test]
fn a_waiting_task_defers_without_penalty() {
    let waiting =
        "T1,Ship it,mit,waiting,p0,2026-08-24,,false,pablo,,0,,,,,2026-08-01,,2026-08-01\n";
    let fixture = fixture(waiting, HABIT);

    let result = defer::defer_in_root(
        &fixture.root,
        &fixture.targets(),
        "T1",
        When::Days(7),
        false,
        today(),
    )
    .expect("defer")
    .0;

    // Waiting on someone else is not avoidance: no count, no demotion.
    assert_eq!(result.no_penalty_reason, Some("waiting"));
    assert_eq!(
        column(&fixture.tasks(), "T1", TASKS_HEADER, "defer_count"),
        "0"
    );
    assert_eq!(
        column(&fixture.tasks(), "T1", TASKS_HEADER, "priority"),
        "p0"
    );
}

#[test]
fn a_blocked_task_defers_without_penalty() {
    let blocked =
        "T1,Ship it,mit,not_started,p0,2026-08-24,,false,pablo,T9,0,,,,,2026-08-01,,2026-08-01\n";
    let fixture = fixture(blocked, HABIT);

    let result = defer::defer_in_root(
        &fixture.root,
        &fixture.targets(),
        "T1",
        When::Days(7),
        false,
        today(),
    )
    .expect("defer")
    .0;

    assert_eq!(result.no_penalty_reason, Some("blocked"));
}

#[test]
fn no_count_forces_a_penalty_free_defer() {
    let fixture = fixture(&task("T1", PLAIN), HABIT);

    let result = defer::defer_in_root(
        &fixture.root,
        &fixture.targets(),
        "T1",
        When::Days(7),
        true,
        today(),
    )
    .expect("defer")
    .0;

    assert_eq!(result.no_penalty_reason, Some("--no-count"));
    assert_eq!(
        column(&fixture.tasks(), "T1", TASKS_HEADER, "defer_count"),
        "0"
    );
}

#[test]
fn later_chunks_cascade_forward_only_when_they_would_invert_the_order() {
    let family = "\
T1,Read spec (1/3),,not_started,p2,2026-08-24,,false,pablo,,0,,,,,2026-08-01,,2026-08-01
T2,Read spec (2/3),,not_started,p2,2026-08-25,,false,pablo,,0,,,,,2026-08-01,,2026-08-01
T3,Read spec (3/3),,not_started,p2,2026-09-30,,false,pablo,,0,,,,,2026-08-01,,2026-08-01
";
    let fixture = fixture(family, HABIT);

    let result = defer::defer_in_root(
        &fixture.root,
        &fixture.targets(),
        "T1",
        When::On(NaiveDate::from_ymd_opt(2026, 9, 1).expect("date")),
        false,
        today(),
    )
    .expect("defer")
    .0;

    // T2 would land before its predecessor, so it moves; T3 is already later.
    assert_eq!(result.cascaded.len(), 1);
    assert_eq!(
        column(&fixture.tasks(), "T2", TASKS_HEADER, "due_date"),
        "2026-09-01"
    );
    assert_eq!(
        column(&fixture.tasks(), "T3", TASKS_HEADER, "due_date"),
        "2026-09-30"
    );
    // A cascade is not the cascaded task's slip.
    assert_eq!(
        column(&fixture.tasks(), "T2", TASKS_HEADER, "defer_count"),
        "0"
    );
}

#[test]
fn a_deferred_task_with_a_tracker_link_reports_it() {
    let linked =
        "T1,Ship it,,not_started,p2,2026-08-24,,false,pablo,,0,,,,ENG-7,2026-08-01,,2026-08-01\n";
    let fixture = fixture(linked, HABIT);

    let result = defer::defer_in_root(
        &fixture.root,
        &fixture.targets(),
        "T1",
        When::Days(1),
        false,
        today(),
    )
    .expect("defer")
    .0;

    // The binary cannot reach an external tracker, so the caller must be told.
    assert_eq!(result.linear_issue.as_deref(), Some("ENG-7"));
}

#[test]
fn a_relative_push_is_parsed_from_the_cli_spelling() {
    assert_eq!(When::parse("+7d").expect("parse"), When::Days(7));
    assert_eq!(
        When::parse("2026-09-15").expect("parse"),
        When::On(NaiveDate::from_ymd_opt(2026, 9, 15).expect("date"))
    );
    assert!(When::parse("next tuesday").is_err());
}

use chrono::NaiveDate;
