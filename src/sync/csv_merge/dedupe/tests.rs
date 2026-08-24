use std::collections::BTreeMap;

use super::dedupe_habit_occurrences;
use crate::sync::csv_merge::{SchemaStatus, Table};

const H: &[&str] = &[
    "task_uuid",
    "task_id",
    "task_name",
    "status",
    "priority",
    "due_date",
    "recur_interval",
    "recur_unit",
    "ideal_time",
    "last_touched",
    "completed_date",
];

fn habit_row(
    uuid: &str,
    task_id: &str,
    name: &str,
    status: &str,
    due_date: &str,
    last_touched: &str,
    completed_date: &str,
) -> Vec<String> {
    [
        uuid,
        task_id,
        name,
        status,
        "p2",
        due_date,
        "1",
        "days",
        "9:00 AM",
        last_touched,
        completed_date,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn tbl(rows: Vec<Vec<String>>) -> Table {
    let header = H.iter().map(|column| (*column).to_owned()).collect();
    let rows = rows
        .into_iter()
        .map(|row| (row[0].clone(), row))
        .collect::<BTreeMap<_, _>>();
    Table {
        header,
        rows,
        schema_status: SchemaStatus::Current,
    }
}

#[test]
fn two_not_started_occurrences_for_the_same_day_collapse_to_one() {
    let mut table = tbl(vec![
        habit_row(
            "10000000-0000-4000-8000-000000000002",
            "H2",
            "Stretch",
            "not_started",
            "2026-01-02",
            "2026-01-01",
            "",
        ),
        habit_row(
            "10000000-0000-4000-8000-000000000001",
            "H1",
            "Stretch",
            "not_started",
            "2026-01-02",
            "2026-01-01",
            "",
        ),
    ]);

    let (removed, notes) = dedupe_habit_occurrences(&mut table);

    assert_eq!(removed, 1);
    assert_eq!(table.rows.len(), 1);
    // The lexicographically smaller uuid survives, deterministically.
    assert!(table.rows.contains_key("10000000-0000-4000-8000-000000000001"));
    assert!(!notes.is_empty());
}

#[test]
fn a_done_occurrence_always_beats_a_not_started_duplicate() {
    let mut table = tbl(vec![
        habit_row(
            "10000000-0000-4000-8000-000000000001",
            "H1",
            "Stretch",
            "done",
            "2026-01-02",
            "2026-01-01",
            "2026-01-02",
        ),
        habit_row(
            "20000000-0000-4000-8000-000000000002",
            "H2",
            "Stretch",
            "not_started",
            "2026-01-02",
            "2026-02-01",
            "",
        ),
    ]);

    let (removed, _notes) = dedupe_habit_occurrences(&mut table);

    assert_eq!(removed, 1);
    assert_eq!(table.rows.len(), 1);
    let survivor = table.rows.values().next().expect("one row remains");
    let status_index = table
        .header
        .iter()
        .position(|column| column == "status")
        .expect("status column");
    let completed_index = table
        .header
        .iter()
        .position(|column| column == "completed_date")
        .expect("completed_date column");
    // Completion wins even though the not-started duplicate has a newer
    // last_touched — same rule as any other merge conflict.
    assert_eq!(survivor[status_index], "done");
    assert_eq!(survivor[completed_index], "2026-01-02");
}

#[test]
fn distinct_occurrences_on_different_days_are_left_alone() {
    let mut table = tbl(vec![
        habit_row(
            "10000000-0000-4000-8000-000000000001",
            "H1",
            "Stretch",
            "done",
            "2026-01-01",
            "2026-01-01",
            "2026-01-01",
        ),
        habit_row(
            "10000000-0000-4000-8000-000000000002",
            "H2",
            "Stretch",
            "not_started",
            "2026-01-02",
            "2026-01-01",
            "",
        ),
    ]);

    let (removed, notes) = dedupe_habit_occurrences(&mut table);

    assert_eq!(removed, 0);
    assert_eq!(table.rows.len(), 2);
    assert!(notes.is_empty());
}

#[test]
fn a_table_with_no_recur_interval_column_is_never_touched() {
    // tasks.csv shape: same task_name/due_date collision would be a real
    // duplicate task, but this pass only ever touches habit-shaped tables.
    let header = [
        "task_uuid",
        "task_id",
        "task_name",
        "status",
        "due_date",
        "last_touched",
    ]
    .iter()
    .map(|column| (*column).to_owned())
    .collect();
    let row_a = vec![
        "10000000-0000-4000-8000-000000000001".to_owned(),
        "T1".to_owned(),
        "Same name".to_owned(),
        "not_started".to_owned(),
        "2026-08-13".to_owned(),
        "2026-08-12".to_owned(),
    ];
    let row_b = vec![
        "10000000-0000-4000-8000-000000000002".to_owned(),
        "T2".to_owned(),
        "Same name".to_owned(),
        "not_started".to_owned(),
        "2026-08-13".to_owned(),
        "2026-08-12".to_owned(),
    ];
    let mut table = Table {
        header,
        rows: BTreeMap::from([
            (row_a[0].clone(), row_a),
            (row_b[0].clone(), row_b),
        ]),
        schema_status: SchemaStatus::Current,
    };

    let (removed, notes) = dedupe_habit_occurrences(&mut table);

    assert_eq!(removed, 0);
    assert_eq!(table.rows.len(), 2);
    assert!(notes.is_empty());
}
