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
fn a_purged_task_leaves_a_breadcrumb_in_its_project() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let root = temporary.path();
    let project = root.join("projects/website");
    std::fs::create_dir_all(&project).expect("project dir");
    std::fs::write(
        project.join(".METADATA.json"),
        r#"{"title":"Website","tasks":["T1","T9"]}"#,
    )
    .expect("metadata");

    let task = purge::PurgedTask {
        task_id: "T1".to_owned(),
        task_name: "Call the vet".to_owned(),
        backlogged_date: "2025-01-01".to_owned(),
        project: "website".to_owned(),
    };
    let found = purge::find_project_dir(root, "website").expect("project located");
    purge::record_in_metadata(&found, &task, today());
    purge::append_breadcrumb(&found, &task, today());

    let metadata = std::fs::read_to_string(project.join(".METADATA.json")).expect("read metadata");
    assert!(metadata.contains("deleted_backlog_tasks"), "{metadata}");
    assert!(
        metadata.contains("\"T9\""),
        "unrelated ids survive:\n{metadata}"
    );
    assert!(
        !metadata.contains("\"tasks\": [\n    \"T1\""),
        "the purged id leaves the live list:\n{metadata}"
    );
    let notes = std::fs::read_to_string(project.join("notes.md")).expect("read notes");
    assert!(notes.contains("## Deleted backlog tasks"), "{notes}");
    assert!(notes.contains("**T1** Call the vet"), "{notes}");
}

#[test]
fn an_archived_project_still_gets_its_breadcrumb() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let root = temporary.path();
    let archived = root.join("archive/2025/website");
    std::fs::create_dir_all(&archived).expect("archived project");
    std::fs::write(archived.join(".METADATA.json"), r#"{"title":"Website"}"#).expect("metadata");

    assert_eq!(purge::find_project_dir(root, "website"), Some(archived));
}

#[test]
fn a_breadcrumb_heading_is_written_once() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let project = temporary.path();
    let task = purge::PurgedTask {
        task_id: "T1".to_owned(),
        task_name: "One".to_owned(),
        backlogged_date: "2025-01-01".to_owned(),
        project: "website".to_owned(),
    };
    purge::append_breadcrumb(project, &task, today());
    purge::append_breadcrumb(
        project,
        &purge::PurgedTask {
            task_id: "T2".to_owned(),
            task_name: "Two".to_owned(),
            ..task
        },
        today(),
    );

    let notes = std::fs::read_to_string(project.join("notes.md")).expect("read notes");
    assert_eq!(
        notes.matches("## Deleted backlog tasks").count(),
        1,
        "{notes}"
    );
    assert!(notes.contains("**T2** Two"), "{notes}");
}
