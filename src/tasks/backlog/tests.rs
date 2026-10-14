//! The backlog's two silent maintenance passes, and the review listing.

use chrono::NaiveDate;

use super::{dedupe, list, minus_six_months, purge};
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

fn parked(id: &str, name: &str, on: &str) -> Row {
    row(&[
        ("task_id", id),
        ("task_name", name),
        ("status", "backlog"),
        ("backlogged_date", on),
        ("priority", "p3"),
    ])
}

#[test]
fn six_months_back_clamps_a_short_month() {
    // 31 Aug − 6 months is 28 Feb, not an invalid 31 Feb.
    assert_eq!(
        minus_six_months(NaiveDate::from_ymd_opt(2026, 8, 31).expect("date")),
        NaiveDate::from_ymd_opt(2026, 2, 28)
    );
    assert_eq!(
        minus_six_months(NaiveDate::from_ymd_opt(2026, 3, 15).expect("date")),
        NaiveDate::from_ymd_opt(2025, 9, 15)
    );
}

#[test]
fn the_review_lists_the_stalest_first() {
    let rows = [
        parked("T1", "Recent", "2026-08-01"),
        parked("T2", "Ancient", "2025-01-01"),
        row(&[
            ("task_id", "T3"),
            ("task_name", "Active"),
            ("status", "not_started"),
        ]),
    ];

    let entries = list::entries(&rows, today());

    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].task_id, "T2");
    assert_eq!(entries[0].days_in_backlog, Some(600));
    assert_eq!(entries[1].task_id, "T1");
}

#[test]
fn a_row_parked_without_a_date_still_lists() {
    let rows = [parked("T1", "Undated", "")];

    let entries = list::entries(&rows, today());

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].days_in_backlog, None);
}

#[test]
fn the_purge_takes_only_what_is_past_the_cutoff() {
    let cutoff = minus_six_months(today()).expect("cutoff");
    let rows = [
        parked("T1", "Ancient", "2025-01-01"),
        parked("T2", "Parked yesterday", "2026-08-23"),
        // Exactly at the cutoff is not *past* it.
        parked("T3", "Borderline", &cutoff.to_string()),
        row(&[
            ("task_id", "T4"),
            ("task_name", "Active"),
            ("status", "not_started"),
            ("backlogged_date", "2020-01-01"),
        ]),
    ];

    let expired = purge::expired(&rows, cutoff);

    assert_eq!(
        expired
            .iter()
            .map(|task| task.task_id.as_str())
            .collect::<Vec<_>>(),
        ["T1"]
    );
}

#[test]
fn dedupe_normalizes_away_case_punctuation_and_spacing() {
    assert_eq!(dedupe::normalize("  Call   the Vet!  "), "call the vet");
    assert_eq!(dedupe::normalize("Re-book: dentist"), "rebook dentist");
}

#[test]
fn a_parked_task_recreated_afterwards_is_superseded() {
    let rows = [
        parked("T1", "Call the vet", "2026-01-01"),
        row(&[
            ("task_id", "T2"),
            ("task_name", "Call the Vet!"),
            ("status", "not_started"),
            ("created_date", "2026-06-01"),
        ]),
    ];

    let superseded = dedupe::superseded(&rows);

    assert_eq!(superseded.len(), 1);
    assert_eq!(superseded[0].task_id, "T1");
}

#[test]
fn a_twin_that_predates_the_parking_is_not_a_re_creation() {
    // The two merely coexisted; the user never chose to revive anything.
    let rows = [
        parked("T1", "Call the vet", "2026-06-01"),
        row(&[
            ("task_id", "T2"),
            ("task_name", "Call the vet"),
            ("status", "not_started"),
            ("created_date", "2026-01-01"),
        ]),
    ];

    assert!(dedupe::superseded(&rows).is_empty());
}

#[test]
fn a_done_twin_does_not_supersede_anything() {
    let rows = [
        parked("T1", "Call the vet", "2026-01-01"),
        row(&[
            ("task_id", "T2"),
            ("task_name", "Call the vet"),
            ("status", "done"),
            ("created_date", "2026-06-01"),
        ]),
    ];

    assert!(dedupe::superseded(&rows).is_empty());
}

#[test]
fn a_reworded_title_is_deliberately_not_a_match() {
    // A false delete is worse than a near-duplicate the caller can catch.
    let rows = [
        parked("T1", "Call the vet", "2026-01-01"),
        row(&[
            ("task_id", "T2"),
            ("task_name", "Ring the vet about Luna"),
            ("status", "not_started"),
            ("created_date", "2026-06-01"),
        ]),
    ];

    assert!(dedupe::superseded(&rows).is_empty());
}

#[test]
fn a_purged_task_leaves_a_breadcrumb_and_history_record() {
    let task = purge::PurgedTask {
        task_id: "T1".into(),
        task_name: "Call the vet".into(),
        backlogged_date: "2025-01-01".into(),
        project: "website".into(),
    };
    let first = purge::append_breadcrumbs("Original", std::slice::from_ref(&task), today());
    let notes = purge::append_breadcrumbs(&first, std::slice::from_ref(&task), today());
    assert_eq!(notes.matches("## Deleted backlog tasks").count(), 1);
    assert!(notes.contains("**T1** Call the vet"));
    let history = purge::history_entry(&task, today());
    assert_eq!(history["task_id"], "T1");
    assert_eq!(history["deleted_date"], today().to_string());
}
