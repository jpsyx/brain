use super::{Edit, set_in_root_with_today};
use crate::tasks::complete::{field, read_csv};
use chrono::NaiveDate;

const TASK_HEADER: &str = "task_uuid,task_id,task_name,task_type,status,priority,due_date,\
                           notes,project,estimated_duration,defer_count,last_touched,\
                           linear_issue,assigned_to,system_key";
const HABIT_HEADER: &str = "task_uuid,task_id,task_name,status,priority,due_date,ideal_time,\
                            recur_interval,recur_unit,completed_date,last_touched,\
                            assigned_to,system_key";

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 8, 7).unwrap()
}

fn workspace(task_rows: &str, habit_rows: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("tasks")).unwrap();
    std::fs::write(
        root.path().join("tasks/tasks.csv"),
        format!("{TASK_HEADER}\n{task_rows}"),
    )
    .unwrap();
    std::fs::write(
        root.path().join("tasks/habits.csv"),
        format!("{HABIT_HEADER}\n{habit_rows}"),
    )
    .unwrap();
    root
}

fn one_task() -> tempfile::TempDir {
    workspace(
        "3f1c9d0e-2b7a-4c55-9f2e-6d1a8b4c7e90,T4,Fix the billing bug,code,not_started,p2,\
         2026-08-10,,,30,1,2026-08-01,AVA-123,pablo,\n",
        "",
    )
}

fn task_row(root: &std::path::Path) -> crate::tasks::complete::Row {
    read_csv(&root.join("tasks/tasks.csv")).unwrap().rows[0].clone()
}

#[test]
fn mirrors_a_due_date_priority_and_title_change_without_touching_defer_count() {
    let root = one_task();
    let edit = Edit {
        name: Some("Fix the billing-side rounding bug".to_owned()),
        due: Some("2026-08-20".to_owned()),
        priority: Some("p1".to_owned()),
        ..Edit::default()
    };

    let plan = set_in_root_with_today(root.path(), "T4", &edit, today()).unwrap();

    assert_eq!(plan.changes.len(), 3);
    let row = task_row(root.path());
    assert_eq!(
        field(&row, "task_name"),
        "Fix the billing-side rounding bug"
    );
    assert_eq!(field(&row, "due_date"), "2026-08-20");
    assert_eq!(field(&row, "priority"), "p1");
    // A tracker-driven reschedule is not the user's slip.
    assert_eq!(field(&row, "defer_count"), "1");
    assert_eq!(field(&row, "last_touched"), "2026-08-07");
}

#[test]
fn resolves_a_task_by_linear_issue_style_fuzzy_name_and_reports_before_after() {
    let root = one_task();
    let edit = Edit {
        priority: Some("P0".to_owned()),
        ..Edit::default()
    };

    let plan = set_in_root_with_today(root.path(), "billing bug", &edit, today()).unwrap();

    assert_eq!(plan.task_id, "T4");
    assert_eq!(plan.changes[0].column, "priority");
    assert_eq!(plan.changes[0].before, "p2");
    assert_eq!(plan.changes[0].after, "p0");
}

#[test]
fn accepts_relative_due_words_and_clearing_the_date() {
    let root = one_task();
    set_in_root_with_today(
        root.path(),
        "T4",
        &Edit {
            due: Some("tomorrow".to_owned()),
            ..Edit::default()
        },
        today(),
    )
    .unwrap();
    assert_eq!(field(&task_row(root.path()), "due_date"), "2026-08-08");

    set_in_root_with_today(
        root.path(),
        "T4",
        &Edit {
            due: Some(String::new()),
            ..Edit::default()
        },
        today(),
    )
    .unwrap();
    assert_eq!(field(&task_row(root.path()), "due_date"), "");
}

#[test]
fn setting_the_same_values_again_is_a_reported_no_op_and_leaves_the_file_untouched() {
    let root = one_task();
    let before = std::fs::read(root.path().join("tasks/tasks.csv")).unwrap();

    let plan = set_in_root_with_today(
        root.path(),
        "T4",
        &Edit {
            priority: Some("p2".to_owned()),
            ..Edit::default()
        },
        today(),
    )
    .unwrap();

    assert!(plan.is_noop());
    assert_eq!(
        std::fs::read(root.path().join("tasks/tasks.csv")).unwrap(),
        before
    );
}

#[test]
fn rejects_an_empty_edit_and_invalid_field_values_without_writing() {
    let root = one_task();
    let before = std::fs::read(root.path().join("tasks/tasks.csv")).unwrap();
    let cases = [
        Edit::default(),
        Edit {
            priority: Some("urgent".to_owned()),
            ..Edit::default()
        },
        Edit {
            status: Some("almost".to_owned()),
            ..Edit::default()
        },
        Edit {
            due: Some("next tuesday".to_owned()),
            ..Edit::default()
        },
        Edit {
            name: Some("   ".to_owned()),
            ..Edit::default()
        },
    ];

    for edit in cases {
        assert!(
            set_in_root_with_today(root.path(), "T4", &edit, today()).is_err(),
            "{edit:?} should be rejected"
        );
    }
    assert_eq!(
        std::fs::read(root.path().join("tasks/tasks.csv")).unwrap(),
        before
    );
}

#[test]
fn editing_a_habit_requires_the_explicit_habit_opt_in() {
    let root = workspace(
        "",
        "8f4ff482-4d40-4a2d-91b1-73ca9f1bfad4,H7,Workout,not_started,p1,2026-08-07,\
         6:45 AM,1,days,,2026-08-01,pablo,\n",
    );
    let before = std::fs::read(root.path().join("tasks/habits.csv")).unwrap();

    let refused = set_in_root_with_today(
        root.path(),
        "H7",
        &Edit {
            due: Some("2026-08-09".to_owned()),
            ..Edit::default()
        },
        today(),
    )
    .unwrap_err()
    .to_string();

    assert!(refused.contains("--habit"), "{refused}");
    assert_eq!(
        std::fs::read(root.path().join("tasks/habits.csv")).unwrap(),
        before
    );

    set_in_root_with_today(
        root.path(),
        "H7",
        &Edit {
            due: Some("2026-08-09".to_owned()),
            ideal_time: Some("7:15 AM".to_owned()),
            habit: true,
            ..Edit::default()
        },
        today(),
    )
    .unwrap();
    let row = read_csv(&root.path().join("tasks/habits.csv"))
        .unwrap()
        .rows[0]
        .clone();
    assert_eq!(field(&row, "due_date"), "2026-08-09");
    assert_eq!(field(&row, "ideal_time"), "7:15 AM");
}

#[test]
fn the_habit_flag_is_refused_for_a_task_and_ideal_time_is_habit_only() {
    let root = one_task();

    assert!(
        set_in_root_with_today(
            root.path(),
            "T4",
            &Edit {
                priority: Some("p1".to_owned()),
                habit: true,
                ..Edit::default()
            },
            today(),
        )
        .is_err()
    );
    assert!(
        set_in_root_with_today(
            root.path(),
            "T4",
            &Edit {
                ideal_time: Some("9:00 AM".to_owned()),
                ..Edit::default()
            },
            today(),
        )
        .is_err()
    );
}

#[test]
fn can_attach_or_repoint_the_mirrored_issue_identifier() {
    let root = one_task();

    set_in_root_with_today(
        root.path(),
        "T4",
        &Edit {
            linear_issue: Some("AVA-456".to_owned()),
            ..Edit::default()
        },
        today(),
    )
    .unwrap();

    assert_eq!(field(&task_row(root.path()), "linear_issue"), "AVA-456");
}
