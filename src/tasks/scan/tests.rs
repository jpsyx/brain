//! The scans triage runs, and the boundaries that keep them from nagging.

use chrono::NaiveDate;

use super::{chronic, linked, waiting};
use crate::tasks::complete::Row;

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 8, 24).expect("valid date")
}

fn row(cells: &[(&str, &str)]) -> Row {
    cells
        .iter()
        .map(|(column, value)| ((*column).to_owned(), (*value).to_owned()))
        .collect()
}

/// A row 30 days untouched, undated: the archetypal chronic hit.
fn stale() -> Row {
    row(&[
        ("task_id", "T1"),
        ("task_name", "Call the vet"),
        ("status", "not_started"),
        ("last_touched", "2026-07-25"),
        ("created_date", "2026-07-01"),
        ("priority", "p3"),
    ])
}

#[test]
fn an_untouched_undated_task_is_chronic() {
    let hit = chronic::classify(&stale(), today()).expect("chronic");
    assert_eq!(hit.reasons, ["stale_21d"]);
    assert_eq!(hit.days_since_touch, Some(30));
}

#[test]
fn a_done_or_parked_task_is_never_chronic() {
    for status in ["done", "backlog"] {
        let mut task = stale();
        task.insert("status".to_owned(), status.to_owned());
        assert!(chronic::classify(&task, today()).is_none(), "{status}");
    }
}

#[test]
fn a_task_not_yet_started_cannot_be_ignored_yet() {
    let mut task = stale();
    task.insert("start_date".to_owned(), "2026-09-01".to_owned());
    assert!(chronic::classify(&task, today()).is_none());
}

#[test]
fn a_past_due_task_belongs_to_past_due_triage() {
    let mut task = stale();
    task.insert("due_date".to_owned(), "2026-08-20".to_owned());
    assert!(chronic::classify(&task, today()).is_none());
}

#[test]
fn a_dated_task_only_surfaces_inside_the_three_day_horizon() {
    let mut inside = stale();
    inside.insert("due_date".to_owned(), "2026-08-27".to_owned());
    assert!(chronic::classify(&inside, today()).is_some());

    let mut outside = stale();
    outside.insert("due_date".to_owned(), "2026-08-28".to_owned());
    assert!(
        chronic::classify(&outside, today()).is_none(),
        "a deadline further out is scheduled, not ignored"
    );
}

#[test]
fn twenty_days_untouched_is_not_yet_stale() {
    let mut task = stale();
    task.insert("last_touched".to_owned(), "2026-08-04".to_owned());
    assert!(chronic::classify(&task, today()).is_none());
}

#[test]
fn an_in_progress_task_goes_stale_a_week_sooner() {
    let mut task = stale();
    task.insert("status".to_owned(), "in_progress".to_owned());
    task.insert("last_touched".to_owned(), "2026-08-08".to_owned());

    let hit = chronic::classify(&task, today()).expect("stuck");

    // 16 days: not yet stale_21d, but the user engaged once and walked away.
    assert_eq!(hit.reasons, ["stuck_in_progress"]);
}

#[test]
fn an_old_thin_never_started_row_is_captured_and_forgotten() {
    let task = row(&[
        ("task_id", "T1"),
        ("task_name", "Something"),
        ("status", "not_started"),
        ("created_date", "2026-05-01"),
        ("last_touched", "2026-08-23"),
    ]);

    let hit = chronic::classify(&task, today()).expect("captured");

    assert_eq!(hit.reasons, ["captured_forgotten"]);
}

#[test]
fn a_fleshed_out_old_row_is_not_captured_and_forgotten() {
    for column in ["notes", "estimated_duration", "project"] {
        let mut task = row(&[
            ("task_id", "T1"),
            ("task_name", "Something"),
            ("status", "not_started"),
            ("created_date", "2026-05-01"),
            ("last_touched", "2026-08-23"),
        ]);
        task.insert((*column).to_owned(), "filled in".to_owned());
        assert!(
            chronic::classify(&task, today()).is_none(),
            "{column} makes it a real row"
        );
    }
}

#[test]
fn chronic_hits_come_back_worst_first() {
    let mut older = stale();
    older.insert("task_id".to_owned(), "T2".to_owned());
    older.insert("last_touched".to_owned(), "2026-01-01".to_owned());

    let hits = chronic::scan(&[stale(), older], today());

    assert_eq!(
        hits.iter()
            .map(|hit| hit.task_id.as_str())
            .collect::<Vec<_>>(),
        ["T2", "T1"]
    );
}

fn waiting_row(id: &str, since: &str) -> Row {
    row(&[
        ("task_id", id),
        ("task_name", "Chase the vendor"),
        ("status", "waiting"),
        ("waiting_since", since),
    ])
}

#[test]
fn a_wait_past_the_threshold_is_flagged() {
    let hit = waiting::classify(&waiting_row("T1", "2026-08-10"), today(), 7).expect("stale wait");
    assert_eq!(hit.days_waiting, Some(14));
}

#[test]
fn a_wait_inside_the_threshold_is_left_alone() {
    assert!(waiting::classify(&waiting_row("T1", "2026-08-20"), today(), 7).is_none());
}

#[test]
fn a_wait_with_no_recorded_start_is_surfaced_anyway() {
    // Not knowing how long it has waited is itself worth fixing.
    let hit = waiting::classify(&waiting_row("T1", ""), today(), 7).expect("unknown wait");
    assert_eq!(hit.days_waiting, None);
}

#[test]
fn stale_waits_sort_longest_first_with_unknowns_last() {
    let rows = [
        waiting_row("T1", "2026-08-10"),
        waiting_row("T2", ""),
        waiting_row("T3", "2026-01-01"),
    ];

    let hits = waiting::scan(&rows, today(), 7);

    assert_eq!(
        hits.iter()
            .map(|hit| hit.task_id.as_str())
            .collect::<Vec<_>>(),
        ["T3", "T1", "T2"]
    );
}

#[test]
fn only_waiting_rows_are_scanned() {
    let mut not_waiting = waiting_row("T1", "2026-01-01");
    not_waiting.insert("status".to_owned(), "in_progress".to_owned());
    assert!(waiting::scan(&[not_waiting], today(), 7).is_empty());
}

#[test]
fn linked_rows_can_be_narrowed_to_the_open_ones() {
    let rows = [
        row(&[
            ("task_id", "T1"),
            ("task_name", "Ship"),
            ("status", "done"),
            ("linear_issue", "ENG-1"),
        ]),
        row(&[
            ("task_id", "T2"),
            ("task_name", "Plan"),
            ("status", "not_started"),
            ("linear_issue", "ENG-2"),
        ]),
        row(&[("task_id", "T3"), ("task_name", "Unlinked")]),
    ];

    assert_eq!(linked::scan(&rows, false).len(), 2);
    let open = linked::scan(&rows, true);
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].linear_issue, "ENG-2");
}
