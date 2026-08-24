//! The rules, and the line between what they fix and what they only flag.

use std::collections::{BTreeMap, BTreeSet};

use chrono::NaiveDate;

use super::{LintReport, links, render, row, run};
use crate::tasks::complete::{CsvFile, Row};

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 8, 24).expect("valid date")
}

fn csv(header: &[&str], rows: Vec<Vec<(&str, &str)>>) -> CsvFile {
    CsvFile {
        header: header.iter().map(|name| (*name).to_owned()).collect(),
        rows: rows
            .into_iter()
            .map(|cells| {
                cells
                    .into_iter()
                    .map(|(column, value)| (column.to_owned(), value.to_owned()))
                    .collect::<Row>()
            })
            .collect(),
    }
}

#[test]
fn a_done_row_without_a_completion_date_is_fixed() {
    let mut file = csv(
        &[
            "task_id",
            "task_name",
            "status",
            "completed_date",
            "last_touched",
        ],
        vec![vec![
            ("task_id", "T1"),
            ("task_name", "Ship"),
            ("status", "done"),
            ("completed_date", ""),
        ]],
    );

    let findings = row::apply(&mut file, "tasks.csv", true, today(), true);

    assert!(findings.changed);
    assert_eq!(
        file.rows[0].get("completed_date").map(String::as_str),
        Some("2026-08-24")
    );
    assert!(findings.fixes[0].0.contains("set completed_date"));
}

#[test]
fn a_dry_run_reports_without_touching_anything() {
    let build = || {
        csv(
            &[
                "task_id",
                "task_name",
                "status",
                "completed_date",
                "last_touched",
            ],
            vec![vec![
                ("task_id", "T1"),
                ("task_name", "Ship"),
                ("status", "done"),
                ("completed_date", ""),
            ]],
        )
    };

    let mut checked = build();
    let dry = row::apply(&mut checked, "tasks.csv", true, today(), false);
    let mut fixed = build();
    let wet = row::apply(&mut fixed, "tasks.csv", true, today(), true);

    assert!(!dry.changed, "a dry run must not mutate");
    assert_eq!(
        checked.rows[0].get("completed_date").map(String::as_str),
        Some("")
    );
    // Everything the fix pass repaired was reported by the check pass first.
    assert!(
        dry.issues
            .iter()
            .any(|issue| issue.0.contains("completed_date"))
    );
    assert!(wet.fixes.iter().any(|fix| fix.0.contains("completed_date")));
}

#[test]
fn an_empty_defer_count_becomes_zero() {
    let mut file = csv(
        &["task_id", "task_name", "defer_count", "last_touched"],
        vec![vec![
            ("task_id", "T1"),
            ("task_name", "Ship"),
            ("defer_count", ""),
        ]],
    );

    row::apply(&mut file, "tasks.csv", true, today(), true);

    assert_eq!(
        file.rows[0].get("defer_count").map(String::as_str),
        Some("0")
    );
}

#[test]
fn a_habit_typed_row_in_the_tasks_table_is_only_flagged() {
    let mut file = csv(
        &["task_id", "task_name", "task_type", "last_touched"],
        vec![vec![
            ("task_id", "T1"),
            ("task_name", "Stretch"),
            ("task_type", "habit"),
        ]],
    );

    let findings = row::apply(&mut file, "tasks.csv", true, today(), true);

    // Moving it between files is a migration, not a lint fix — so it is
    // reported even in the pass that fixes everything it can.
    assert!(
        !findings
            .fixes
            .iter()
            .any(|fix| fix.0.contains("habits.csv")),
        "{findings:?}"
    );
    assert!(
        findings
            .issues
            .iter()
            .any(|issue| issue.0.contains("should move to habits.csv")),
        "{findings:?}"
    );
}

#[test]
fn the_same_row_in_the_habits_table_is_fine() {
    let mut file = csv(
        &["task_id", "task_name", "task_type", "last_touched"],
        vec![vec![
            ("task_id", "H1"),
            ("task_name", "Stretch"),
            ("task_type", "habit"),
        ]],
    );

    assert!(
        row::apply(&mut file, "habits.csv", false, today(), true)
            .issues
            .is_empty()
    );
}

#[test]
fn sub_task_checkboxes_are_detected_in_any_indentation() {
    assert!(row::has_checkboxes("- [ ] one"));
    assert!(row::has_checkboxes("intro\n   - [x] done"));
    assert!(!row::has_checkboxes("a list:\n- one\n- two"));
    assert!(!row::has_checkboxes("see [the doc]"));
}

#[test]
fn a_missing_last_touched_column_is_added_and_backfilled() {
    let mut file = csv(
        &["task_id", "task_name", "created_date"],
        vec![
            vec![
                ("task_id", "T1"),
                ("task_name", "Ship"),
                ("created_date", "2026-01-05"),
            ],
            vec![("task_id", "T2"), ("task_name", "No date")],
        ],
    );

    row::apply(&mut file, "tasks.csv", true, today(), true);

    assert!(file.header.iter().any(|column| column == "last_touched"));
    assert_eq!(
        file.rows[0].get("last_touched").map(String::as_str),
        Some("2026-01-05"),
        "backfilled from created_date"
    );
    assert_eq!(
        file.rows[1].get("last_touched").map(String::as_str),
        Some("2026-08-24"),
        "no created_date falls back to today"
    );
}

fn ids(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|id| (*id).to_owned()).collect()
}

#[test]
fn a_project_that_does_not_know_its_task_is_repaired() {
    let forward = BTreeMap::from([("website".to_owned(), ids(&["T1", "T2"]))]);
    let projects = [links::ProjectLinks {
        slug: "website".to_owned(),
        listed: ids(&["T1"]),
    }];

    let found = links::reconcile(&forward, &ids(&["website"]), &projects);

    // The task's claim is the newer fact, so the metadata catches up.
    assert_eq!(found.repairs["website"], ids(&["T1", "T2"]));
    assert!(found.issues.is_empty());
}

#[test]
fn a_project_listing_a_task_that_does_not_exist_is_only_reported() {
    let forward = BTreeMap::new();
    let projects = [links::ProjectLinks {
        slug: "website".to_owned(),
        listed: ids(&["T9"]),
    }];

    let found = links::reconcile(&forward, &ids(&["website"]), &projects);

    // Something was deleted or renamed; guessing which would destroy data.
    assert!(found.repairs.is_empty());
    assert!(found.issues[0].0.contains("orphan project→task"));
}

#[test]
fn a_task_pointing_at_a_missing_project_is_reported() {
    let forward = BTreeMap::from([("gone".to_owned(), ids(&["T1"]))]);

    let found = links::reconcile(&forward, &BTreeSet::new(), &[]);

    assert!(found.issues[0].0.contains("orphan task→project"));
}

#[test]
fn a_whole_workspace_lints_and_repairs_its_project_metadata() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let root = temporary.path();
    std::fs::create_dir_all(root.join("tasks")).expect("tasks dir");
    std::fs::create_dir_all(root.join("projects/website")).expect("project dir");
    std::fs::write(
        root.join("tasks/tasks.csv"),
        "task_id,task_name,status,completed_date,project,created_date\n\
T1,Ship,done,,website,2026-01-05\n",
    )
    .expect("tasks.csv");
    std::fs::write(
        root.join("projects/website/.METADATA.json"),
        r#"{"name":"website","tasks":[]}"#,
    )
    .expect("metadata");

    let report = run(root, today(), true).expect("lint");

    assert!(report.issues.is_empty(), "{report:?}");
    let tasks = std::fs::read_to_string(root.join("tasks/tasks.csv")).expect("read tasks");
    assert!(tasks.contains("2026-08-24"), "{tasks}");
    let metadata =
        std::fs::read_to_string(root.join("projects/website/.METADATA.json")).expect("metadata");
    assert!(metadata.contains("\"T1\""), "{metadata}");
}

#[test]
fn a_clean_workspace_says_so() {
    let report = LintReport::default();
    assert_eq!(
        render(&report, false, crate::theme::Theme::dark(false)),
        "Task rules: all clean.\n"
    );
}

#[test]
fn a_dry_run_with_issues_points_at_the_fix_flag() {
    let report = LintReport {
        fixes: Vec::new(),
        issues: vec!["something".to_owned()],
    };
    let out = render(&report, false, crate::theme::Theme::dark(false));
    assert!(out.contains("1 issue(s):"), "{out}");
    assert!(out.contains("run with --fix"), "{out}");
}
