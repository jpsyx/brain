use chrono::NaiveDate;

use super::cleanup;
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

fn done_on(id: &str, date: &str) -> Row {
    row(&[
        ("task_id", id),
        ("task_name", "Stretch"),
        ("status", "done"),
        ("completed_date", date),
    ])
}

#[test]
fn completions_older_than_a_week_are_swept() {
    let rows = [done_on("H1", "2026-08-01"), done_on("H2", "2026-08-17")];

    let plan = cleanup::plan(&rows, today(), true);

    // 17 Aug is exactly the cutoff, and the cutoff itself is swept.
    assert_eq!(plan.dropped, ["H1", "H2"]);
    assert_eq!(
        plan.cutoff,
        NaiveDate::from_ymd_opt(2026, 8, 17).expect("date")
    );
}

#[test]
fn a_recent_completion_stays_for_inspection() {
    let plan = cleanup::plan(&[done_on("H1", "2026-08-20")], today(), true);
    assert!(plan.dropped.is_empty());
    assert_eq!(plan.kept, 1);
}

#[test]
fn a_pending_occurrence_is_never_swept() {
    let rows = [row(&[
        ("task_id", "H1"),
        ("task_name", "Stretch"),
        ("status", "not_started"),
        ("completed_date", "2020-01-01"),
    ])];

    assert!(cleanup::plan(&rows, today(), true).dropped.is_empty());
}

#[test]
fn a_managed_triage_row_is_never_swept_by_retention() {
    let mut managed = done_on("H9", "2020-01-01");
    managed.insert("system_key".to_owned(), "brain.triage.daily".to_owned());

    let plan = cleanup::plan(&[managed], today(), true);

    // Removing a managed row is a transactional decision, not a retention one.
    assert!(plan.dropped.is_empty());
    assert_eq!(plan.deferred_managed, 0);
}

#[test]
fn a_managed_row_left_behind_by_a_disabled_feature_is_counted() {
    let mut managed = done_on("H9", "2020-01-01");
    managed.insert("system_key".to_owned(), "brain.triage.weekly".to_owned());

    let plan = cleanup::plan(&[managed], today(), false);

    assert_eq!(plan.deferred_managed, 1);
    assert!(plan.dropped.is_empty());
}
