use super::{CreateRequest, create_in_root_for_actor_with_today};
use crate::actor::ActorContext;
use crate::tasks::complete::{field, read_csv};
use chrono::NaiveDate;
use tempfile::tempdir;

#[test]
fn creates_email_follow_up_with_stable_id_and_metadata() {
    let root = tempdir().unwrap();
    std::fs::create_dir(root.path().join("tasks")).unwrap();
    let actor: ActorContext =
        serde_json::from_str(r#"{"user_id":"pablo","display_name":"Pablo","channel":"email"}"#)
            .unwrap();
    let request = CreateRequest {
        name: "Reply: Alex / launch".to_owned(),
        task_type: Some("personal|needs_attention".to_owned()),
        priority: "p1".to_owned(),
        due: Some("2026-08-07".to_owned()),
        notes: Some("https://superhuman.local/thread | reply today".to_owned()),
        ..CreateRequest::default()
    };

    let result = create_in_root_for_actor_with_today(
        root.path(),
        &actor,
        &request,
        NaiveDate::from_ymd_opt(2026, 8, 7).unwrap(),
    )
    .unwrap();

    assert_eq!(result.ids().into_iter().collect::<Vec<_>>(), vec!["T1"]);
    let csv = read_csv(&root.path().join("tasks/tasks.csv")).unwrap();
    let row = &csv.rows[0];
    assert_eq!(field(row, "task_type"), "personal|needs_attention");
    assert_eq!(field(row, "assigned_to"), "pablo");
    assert_eq!(
        field(row, "notes"),
        "https://superhuman.local/thread | reply today"
    );
}

#[test]
fn creates_habit_and_chunked_tasks_with_native_counters() {
    let root = tempdir().unwrap();
    std::fs::create_dir(root.path().join("tasks")).unwrap();
    let actor: ActorContext = serde_json::from_str(
        r#"{"user_id":"pablo","display_name":"Pablo","channel":"interactive"}"#,
    )
    .unwrap();
    let today = NaiveDate::from_ymd_opt(2026, 8, 7).unwrap();
    let habit = CreateRequest {
        name: "Walk".to_owned(),
        priority: "p2".to_owned(),
        habit: true,
        interval: Some(1),
        unit: Some("days".to_owned()),
        ..CreateRequest::default()
    };
    assert_eq!(
        create_in_root_for_actor_with_today(root.path(), &actor, &habit, today)
            .unwrap()
            .ids()
            .into_iter()
            .collect::<Vec<_>>(),
        vec!["H1"]
    );

    let chunks = CreateRequest {
        name: "Draft reply".to_owned(),
        task_type: Some("code|mit".to_owned()),
        priority: "p1".to_owned(),
        duration: Some("30".to_owned()),
        chunks: Some(2),
        linear_issue: Some("AVA-123".to_owned()),
        ..CreateRequest::default()
    };
    let result = create_in_root_for_actor_with_today(root.path(), &actor, &chunks, today).unwrap();
    assert_eq!(
        result.ids().into_iter().collect::<Vec<_>>(),
        vec!["T1", "T2"]
    );
    let csv = read_csv(&root.path().join("tasks/tasks.csv")).unwrap();
    assert_eq!(field(&csv.rows[0], "task_type"), "code|mit");
    assert_eq!(field(&csv.rows[1], "task_type"), "code");
    assert_eq!(field(&csv.rows[1], "blocked_by"), "T1");
    assert_eq!(field(&csv.rows[0], "linear_issue"), "AVA-123");
    assert_eq!(field(&csv.rows[1], "linear_issue"), "");
    assert_ne!(
        field(&csv.rows[0], "task_uuid"),
        field(&csv.rows[1], "task_uuid")
    );
}

#[test]
fn explicit_assignment_requires_a_workspace_member() {
    let root = tempdir().unwrap();
    std::fs::create_dir(root.path().join("tasks")).unwrap();
    std::fs::create_dir(root.path().join(".config")).unwrap();
    std::fs::write(root.path().join(".config/users.json"), r#"{"schema_version":1,"users":[{"id":"alex","name":"Alex","phones":[],"emails":[],"response_email":null}]}"#).unwrap();
    let actor: ActorContext = serde_json::from_str(
        r#"{"user_id":"pablo","display_name":"Pablo","channel":"interactive"}"#,
    )
    .unwrap();
    let request = CreateRequest {
        name: "Assigned".to_owned(),
        task_type: Some("personal".to_owned()),
        priority: "p1".to_owned(),
        assigned_to: Some("alex".to_owned()),
        ..CreateRequest::default()
    };
    create_in_root_for_actor_with_today(
        root.path(),
        &actor,
        &request,
        NaiveDate::from_ymd_opt(2026, 8, 7).unwrap(),
    )
    .unwrap();
    let csv = read_csv(&root.path().join("tasks/tasks.csv")).unwrap();
    assert_eq!(field(&csv.rows[0], "assigned_to"), "alex");
}

#[test]
fn validates_habit_and_chunk_constraints_before_writing() {
    let root = tempdir().unwrap();
    let actor: ActorContext = serde_json::from_str(
        r#"{"user_id":"pablo","display_name":"Pablo","channel":"interactive"}"#,
    )
    .unwrap();
    let request = CreateRequest {
        name: "Bad".to_owned(),
        priority: "p1".to_owned(),
        habit: true,
        chunks: Some(2),
        ..CreateRequest::default()
    };
    let error = create_in_root_for_actor_with_today(
        root.path(),
        &actor,
        &request,
        NaiveDate::from_ymd_opt(2026, 8, 7).unwrap(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("--chunks is not supported"));
    assert!(!root.path().join("tasks").exists());
}

#[test]
fn a_created_habit_records_its_ideal_time_for_time_of_day_grouping() {
    let root = tempdir().unwrap();
    std::fs::create_dir(root.path().join("tasks")).unwrap();
    let actor: ActorContext = serde_json::from_str(
        r#"{"user_id":"pablo","display_name":"Pablo","channel":"interactive"}"#,
    )
    .unwrap();
    let today = NaiveDate::from_ymd_opt(2026, 8, 7).unwrap();
    let request = CreateRequest {
        name: "Workout".to_owned(),
        priority: "p1".to_owned(),
        habit: true,
        interval: Some(1),
        unit: Some("days".to_owned()),
        ideal_time: Some("6:45 AM".to_owned()),
        ..CreateRequest::default()
    };

    create_in_root_for_actor_with_today(root.path(), &actor, &request, today).unwrap();

    let csv = read_csv(&root.path().join("tasks/habits.csv")).unwrap();
    assert!(csv.header.iter().any(|column| column == "ideal_time"));
    assert_eq!(field(&csv.rows[0], "ideal_time"), "6:45 AM");
}

#[test]
fn ideal_time_is_rejected_for_a_plain_task() {
    let root = tempdir().unwrap();
    std::fs::create_dir(root.path().join("tasks")).unwrap();
    let actor: ActorContext = serde_json::from_str(
        r#"{"user_id":"pablo","display_name":"Pablo","channel":"interactive"}"#,
    )
    .unwrap();
    let today = NaiveDate::from_ymd_opt(2026, 8, 7).unwrap();
    let request = CreateRequest {
        name: "File receipts".to_owned(),
        task_type: Some("personal".to_owned()),
        priority: "p2".to_owned(),
        ideal_time: Some("6:45 AM".to_owned()),
        ..CreateRequest::default()
    };

    let error = create_in_root_for_actor_with_today(root.path(), &actor, &request, today)
        .unwrap_err()
        .to_string();

    assert!(error.contains("--ideal-time"), "{error}");
}
