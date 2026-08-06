use std::cell::RefCell;

use brain::workspace::{WorkspaceId, WorkspacePaths};

const WORKSPACE_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";

#[test]
fn transition_publishes_current_csvs_then_baselines_then_schema_metadata() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("workspace");
    write_current_task_state(&root);
    let paths = WorkspacePaths::new(temporary.path(), WorkspaceId::parse(WORKSPACE_ID).unwrap());
    let published = RefCell::new(Vec::new());

    brain::migration::publish_task_schema_transition_with_transport(
        &paths,
        &root,
        None,
        |relative, _bytes| {
            if relative == "tasks/SCHEMA.json" {
                assert_eq!(
                    std::fs::read(paths.sync_csv_baselines().join("tasks.csv")).unwrap(),
                    std::fs::read(root.join("tasks/tasks.csv")).unwrap()
                );
                assert_eq!(
                    std::fs::read(paths.sync_csv_baselines().join("habits.csv")).unwrap(),
                    std::fs::read(root.join("tasks/habits.csv")).unwrap()
                );
            }
            published.borrow_mut().push(relative.to_owned());
            true
        },
    )
    .unwrap();

    assert_eq!(
        *published.borrow(),
        ["tasks/tasks.csv", "tasks/habits.csv", "tasks/SCHEMA.json"]
    );
}

#[test]
fn transition_failure_before_both_csvs_publish_leaves_schema_and_baselines_unpublished() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("workspace");
    write_current_task_state(&root);
    let paths = WorkspacePaths::new(temporary.path(), WorkspaceId::parse(WORKSPACE_ID).unwrap());
    let published = RefCell::new(Vec::new());

    let error = brain::migration::publish_task_schema_transition_with_transport(
        &paths,
        &root,
        None,
        |relative, _bytes| {
            published.borrow_mut().push(relative.to_owned());
            relative != "tasks/habits.csv"
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("tasks/habits.csv"), "{error:#}");
    assert_eq!(*published.borrow(), ["tasks/tasks.csv", "tasks/habits.csv"]);
    assert!(!paths.sync_csv_baselines().exists());
}

#[test]
fn transition_refuses_a_newer_remote_schema_before_any_publication() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("workspace");
    write_current_task_state(&root);
    let paths = WorkspacePaths::new(temporary.path(), WorkspaceId::parse(WORKSPACE_ID).unwrap());
    let published = RefCell::new(Vec::new());

    let error = brain::migration::publish_task_schema_transition_with_transport(
        &paths,
        &root,
        Some(r#"{"task_schema_version":3,"merge_key":"task_uuid"}"#),
        |relative, _bytes| {
            published.borrow_mut().push(relative.to_owned());
            true
        },
    )
    .unwrap_err();

    assert!(
        error.to_string().contains("remote task schema version 3"),
        "{error:#}"
    );
    assert!(published.borrow().is_empty());
    assert!(!paths.sync_csv_baselines().exists());
}

fn write_current_task_state(root: &std::path::Path) {
    let tasks = root.join("tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    std::fs::write(
        tasks.join("tasks.csv"),
        "task_uuid,task_id,assigned_to,system_key\n10000000-0000-4000-8000-000000000001,T1,pablo,\n",
    )
    .unwrap();
    std::fs::write(
        tasks.join("habits.csv"),
        "task_uuid,task_id,assigned_to,system_key\n10000000-0000-4000-8000-000000000002,H1,pablo,\n",
    )
    .unwrap();
    std::fs::write(
        tasks.join("SCHEMA.json"),
        b"{\"task_schema_version\":2,\"merge_key\":\"task_uuid\",\"display_identity\":{\"field\":\"task_id\",\"mutable\":true}}\n",
    )
    .unwrap();
}
