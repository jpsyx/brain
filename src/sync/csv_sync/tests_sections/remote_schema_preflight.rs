
#[test]
fn newer_remote_task_schema_refuses_merge_before_any_csv_publication() {
    use std::cell::Cell;
    use std::collections::BTreeMap;

    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("workspace");
    let tasks = root.join("tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    let task_text = "task_uuid,task_id,assigned_to,system_key\n\
                     10000000-0000-4000-8000-000000000010,T10,member-a,\n";
    let habit_text = "task_uuid,task_id,assigned_to,system_key\n\
                      20000000-0000-4000-8000-000000000010,H10,member-a,\n";
    std::fs::write(tasks.join("tasks.csv"), task_text).unwrap();
    std::fs::write(tasks.join("habits.csv"), habit_text).unwrap();
    std::fs::write(
        tasks.join("SCHEMA.json"),
        r#"{"task_schema_version":2,"merge_key":"task_uuid","display_identity":{"field":"task_id","mutable":true}}"#,
    )
    .unwrap();
    let remote = BTreeMap::from([
        ("tasks/tasks.csv", task_text),
        ("tasks/habits.csv", habit_text),
        (
            "tasks/SCHEMA.json",
            r#"{"task_schema_version":3,"merge_key":"task_uuid"}"#,
        ),
    ]);
    let pushes = Cell::new(0);

    let result = sync_csvs_with_transport(
        &paths(directory.path()),
        &root,
        Direction::Both,
        |relative| remote.get(relative).map(ToString::to_string),
        |_, _| {
            pushes.set(pushes.get() + 1);
            true
        },
    );

    let error = result.unwrap_err();
    assert!(
        error.to_string().contains("remote task schema version 3"),
        "{error:#}"
    );
    assert_eq!(pushes.get(), 0);
}

#[test]
fn malformed_or_incompatible_remote_task_schema_refuses_all_publication() {
    use std::cell::Cell;

    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("workspace");
    let tasks = root.join("tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    let task_text = "task_uuid,task_id,assigned_to,system_key\n\
                     10000000-0000-4000-8000-000000000010,T10,member-a,\n";
    let habit_text = "task_uuid,task_id,assigned_to,system_key\n\
                      20000000-0000-4000-8000-000000000010,H10,member-a,\n";
    let local_schema = r#"{"task_schema_version":2,"merge_key":"task_uuid","display_identity":{"field":"task_id","mutable":true}}"#;
    std::fs::write(tasks.join("tasks.csv"), task_text).unwrap();
    std::fs::write(tasks.join("habits.csv"), habit_text).unwrap();
    std::fs::write(tasks.join("SCHEMA.json"), local_schema).unwrap();

    for remote_schema in [
        "not-json",
        r#"{"task_schema_version":2,"merge_key":"task_id"}"#,
    ] {
        let pushes = Cell::new(0);
        let result = sync_csvs_with_transport(
            &paths(directory.path()),
            &root,
            Direction::Both,
            |relative| match relative {
                "tasks/tasks.csv" => Some(task_text.to_owned()),
                "tasks/habits.csv" => Some(habit_text.to_owned()),
                "tasks/SCHEMA.json" => Some(remote_schema.to_owned()),
                _ => None,
            },
            |_, _| {
                pushes.set(pushes.get() + 1);
                true
            },
        );

        let error = result.unwrap_err();
        assert!(error.to_string().contains("remote"), "{error:#}");
        assert_eq!(pushes.get(), 0);
    }
}

#[test]
fn present_wrong_typed_remote_schema_is_not_legacy_and_cannot_publish() {
    use std::cell::Cell;

    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("workspace");
    let tasks = root.join("tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    let task_text = "task_id,status\nT1,open\n";
    let habit_text = "task_id,status\nH1,open\n";
    std::fs::write(tasks.join("tasks.csv"), task_text).unwrap();
    std::fs::write(tasks.join("habits.csv"), habit_text).unwrap();
    std::fs::write(tasks.join("SCHEMA.json"), "{}\n").unwrap();
    let pushes = Cell::new(0);

    let error = sync_csvs_with_transport(
        &paths(directory.path()),
        &root,
        Direction::Both,
        |relative| match relative {
            "tasks/tasks.csv" => Some(task_text.to_owned()),
            "tasks/habits.csv" => Some(habit_text.to_owned()),
            "tasks/SCHEMA.json" => {
                Some(r#"{"task_schema_version":"3","merge_key":"task_uuid"}"#.to_owned())
            }
            _ => None,
        },
        |_, _| {
            pushes.set(pushes.get() + 1);
            true
        },
    )
    .unwrap_err();

    assert!(
        error.to_string().contains("remote task_schema_version"),
        "{error:#}"
    );
    assert_eq!(pushes.get(), 0);
    assert!(!baseline_path(&paths(directory.path()), "tasks.csv").exists());
}

#[test]
fn absent_remote_schema_is_legacy_only_and_never_implicitly_current() {
    use std::cell::Cell;

    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("workspace");
    let tasks = root.join("tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    let task_text = "task_id,status\nT1,open\n";
    let habit_text = "task_id,status\nH1,open\n";
    std::fs::write(tasks.join("tasks.csv"), task_text).unwrap();
    std::fs::write(tasks.join("habits.csv"), habit_text).unwrap();
    std::fs::write(tasks.join("SCHEMA.json"), "{}\n").unwrap();
    let fetch = |relative: &str| match relative {
        "tasks/tasks.csv" => Some(task_text.to_owned()),
        "tasks/habits.csv" => Some(habit_text.to_owned()),
        _ => None,
    };

    sync_csvs_with_transport(
        &paths(directory.path()),
        &root,
        Direction::Both,
        fetch,
        |_, _| true,
    )
    .unwrap();

    std::fs::write(
        tasks.join("SCHEMA.json"),
        r#"{"task_schema_version":2,"merge_key":"task_uuid","display_identity":{"field":"task_id","mutable":true}}"#,
    )
    .unwrap();
    let pushes = Cell::new(0);
    let error = sync_csvs_with_transport(
        &paths(directory.path()),
        &root,
        Direction::Both,
        fetch,
        |_, _| {
            pushes.set(pushes.get() + 1);
            true
        },
    )
    .unwrap_err();

    assert!(
        error.to_string().contains("remote task schema is Legacy"),
        "{error:#}"
    );
    assert_eq!(pushes.get(), 0);
}
