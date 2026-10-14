use serde_json::json;

use super::fixture::{csv, needles, row};
use crate::tasks::identity::CsvKind;
use crate::tasks::removal::model::Options;
use crate::tasks::removal::resolve::{resolve_for_removal, resolve_targets};

#[test]
fn removal_fingerprint_changes_only_with_canonical_input() {
    use crate::tasks::removal::fingerprint::digest;
    use std::collections::BTreeMap;

    let a = BTreeMap::from([
        ("tasks/tasks.csv".to_owned(), b"before".to_vec()),
        ("tasks/habits.csv".to_owned(), Vec::new()),
    ]);
    let mut b = BTreeMap::new();
    b.insert("tasks/habits.csv".to_owned(), Vec::new());
    b.insert("tasks/tasks.csv".to_owned(), b"before".to_vec());
    assert_eq!(digest(&a).unwrap(), digest(&b).unwrap());
    b.insert("tasks/tasks.csv".into(), b"after".to_vec());
    assert_ne!(digest(&a).unwrap(), digest(&b).unwrap());
}

#[test]
fn targets_sort_by_kind_then_numeric_id() {
    let tasks = csv(vec![row("T17", "Send proposal"), row("T2", "Book meeting")]);
    let habits = csv(vec![row("H10", "Read"), row("H2", "Walk")]);
    let result = resolve_targets(&tasks, &habits, &needles(&["T17", "H10", "T2", "H2"]));
    assert_eq!(
        result.expect("targets"),
        vec![
            (CsvKind::Habits, 1),
            (CsvKind::Habits, 0),
            (CsvKind::Tasks, 1),
            (CsvKind::Tasks, 0),
        ]
    );
}

#[test]
fn bare_number_ambiguity_reports_original_needle_and_both_rows() {
    let tasks = csv(vec![row("T17", "Send proposal")]);
    let habits = csv(vec![row("H17", "Walk")]);
    let problem = resolve_targets(&tasks, &habits, &needles(&[" 017 "])).expect_err("ambiguous");
    assert_eq!(problem.code, "ambiguous_target");
    assert_eq!(
        problem.details,
        json!({
            "needle": " 017 ",
            "candidates": [
                {"task_id": "H17", "task_name": "Walk"},
                {"task_id": "T17", "task_name": "Send proposal"},
            ],
        })
    );
}

#[test]
fn ambiguous_name_reports_matching_candidates_only() {
    let tasks = csv(vec![
        row("T17", "Send proposal"),
        row("T2", "Review proposal"),
        row("T3", "Book meeting"),
    ]);
    let problem =
        resolve_targets(&tasks, &csv(vec![]), &needles(&["PROPOSAL"])).expect_err("ambiguous");
    assert_eq!(problem.code, "ambiguous_target");
    assert_eq!(
        problem.details,
        json!({
            "needle": "PROPOSAL", "candidates": [
                {"task_id": "T2", "task_name": "Review proposal"},
                {"task_id": "T17", "task_name": "Send proposal"},
            ],
        })
    );
}

#[test]
fn unique_name_fragment_and_display_aliases_resolve_one_row() {
    let tasks = csv(vec![row("T17", "Send proposal"), row("T2", "Book meeting")]);
    assert_eq!(
        resolve_targets(&tasks, &csv(vec![]), &needles(&["PROPOSAL", "t017", "17"]))
            .expect("targets"),
        vec![(CsvKind::Tasks, 0)]
    );
}

#[test]
fn one_missing_target_rejects_the_entire_batch() {
    let tasks = csv(vec![row("T17", "Send proposal")]);
    let problem = resolve_targets(&tasks, &csv(vec![]), &needles(&["T17", "T99"]))
        .expect_err("missing target");
    assert_eq!(problem.code, "target_not_found");
    assert_eq!(problem.details, json!({"needle": "T99", "candidates": []}));
}

#[test]
fn invalid_persisted_uuid_rejects_the_batch_without_schema_changes() {
    let mut invalid = row("T17", "Send proposal");
    invalid.insert("task_uuid".into(), "not-a-uuid".into());
    let tasks = csv(vec![row("T2", "Book meeting"), invalid]);
    let before = tasks.clone();
    let problem =
        resolve_targets(&tasks, &csv(vec![]), &needles(&["T2", "T17"])).expect_err("invalid UUID");
    assert_eq!(problem.code, "invalid_task_uuid");
    assert_eq!(problem.details["task_id"], "T17");
    assert_eq!(tasks.header, before.header);
    assert_eq!(tasks.rows, before.rows);
}

#[test]
fn mixed_tasks_and_habits_are_rejected() {
    let problem = resolve_for_removal(
        &csv(vec![row("T17", "Send proposal")]),
        &csv(vec![row("H2", "Walk")]),
        &needles(&["T17", "H2"]),
        &Options::default(),
        true,
    )
    .expect_err("mixed batch");
    assert_eq!(problem.code, "mixed_target_kinds");
}

#[test]
fn habit_requires_explicit_opt_in() {
    let problem = resolve_for_removal(
        &csv(vec![]),
        &csv(vec![row("H2", "Walk")]),
        &needles(&["H2"]),
        &Options::default(),
        true,
    )
    .expect_err("habit requires flag");
    assert_eq!(problem.code, "habit_required");
}

#[test]
fn habit_flag_on_task_is_rejected() {
    let options = Options {
        habit: true,
        ..Options::default()
    };
    let problem = resolve_for_removal(
        &csv(vec![row("T17", "Send proposal")]),
        &csv(vec![]),
        &needles(&["T17"]),
        &options,
        true,
    )
    .expect_err("task rejects flag");
    assert_eq!(problem.code, "habit_flag_on_task");
}

#[test]
fn enabled_managed_habits_are_protected_even_with_opt_in() {
    let options = Options {
        habit: true,
        ..Options::default()
    };
    for key in ["brain.triage.daily", " brain.triage.weekly "] {
        let mut habit = row("H2", "Review");
        habit.insert("system_key".into(), key.into());
        let problem = resolve_for_removal(
            &csv(vec![]),
            &csv(vec![habit]),
            &needles(&["H2"]),
            &options,
            true,
        )
        .expect_err("managed habit");
        assert_eq!(problem.code, "managed_task_cannot_delete");
    }
}

#[test]
fn disabled_managed_habit_and_ordinary_habit_allow_opt_in() {
    let options = Options {
        habit: true,
        ..Options::default()
    };
    for (key, enabled) in [("brain.triage.daily", false), ("user.review", true)] {
        let mut habit = row("H2", "Review");
        habit.insert("system_key".into(), key.into());
        assert_eq!(
            resolve_for_removal(
                &csv(vec![]),
                &csv(vec![habit]),
                &needles(&["H2"]),
                &options,
                enabled
            )
            .expect("ordinary habit"),
            vec![(CsvKind::Habits, 0)]
        );
    }
}

#[test]
fn rewiring_a_removed_middle_task_preserves_its_prerequisite() {
    use crate::tasks::removal::{dependencies::rewrite, model::Dependents};
    use std::collections::{BTreeMap, BTreeSet};
    let graph = BTreeMap::from([
        ("T1".into(), BTreeSet::new()),
        ("T2".into(), BTreeSet::from(["T1".into()])),
        ("T3".into(), BTreeSet::from(["T2".into()])),
    ]);
    let result = rewrite(&graph, &BTreeSet::from(["T2".into()]), Dependents::Rewire).unwrap();
    assert_eq!(result["T3"], BTreeSet::from(["T1".into()]));
    assert!(!result.contains_key("T2"));
}
