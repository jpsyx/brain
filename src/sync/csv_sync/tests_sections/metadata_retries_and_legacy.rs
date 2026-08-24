
#[test]
fn metadata_retry_republishes_locally_unchanged_authoritative_files() {
    use std::cell::{Cell, RefCell};
    use std::collections::BTreeMap;

    let directory = tempfile::tempdir().unwrap();
    for project in ["alpha", "beta"] {
        let metadata = directory
            .path()
            .join(format!("projects/{project}/.METADATA.json"));
        std::fs::create_dir_all(metadata.parent().unwrap()).unwrap();
        std::fs::write(
            metadata,
            format!("{{\n  \"name\": \"{project}\",\n  \"tasks\": []\n}}\n"),
        )
        .unwrap();
    }
    let remote = RefCell::new(BTreeMap::<String, String>::new());
    let calls = Cell::new(0);

    let paths = paths(directory.path());
    let first = sync_csvs_with_transport(
        &paths,
        directory.path(),
        Direction::Both,
        |_| None,
        |relative, text| {
            calls.set(calls.get() + 1);
            if calls.get() == 2 {
                return false;
            }
            remote
                .borrow_mut()
                .insert(relative.to_owned(), text.to_owned());
            true
        },
    );
    assert!(matches!(first, Err(CsvSyncError::RemotePublish(_))));
    assert_eq!(remote.borrow().len(), 1);

    let second = sync_csvs_with_transport(
        &paths,
        directory.path(),
        Direction::Both,
        |_| None,
        |relative, text| {
            remote
                .borrow_mut()
                .insert(relative.to_owned(), text.to_owned());
            true
        },
    );

    assert!(second.is_ok());
    for project in ["alpha", "beta"] {
        let relative = format!("projects/{project}/.METADATA.json");
        let local = std::fs::read_to_string(directory.path().join(&relative)).unwrap();
        assert_eq!(remote.borrow().get(&relative), Some(&local));
    }
}

#[test]
fn one_invalid_csv_preflight_blocks_both_csvs_baselines_and_metadata() {
    use std::cell::Cell;
    use std::collections::BTreeMap;

    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("workspace");
    let tasks_dir = root.join("tasks");
    std::fs::create_dir_all(&tasks_dir).unwrap();
    let tasks = "task_uuid,task_id,assigned_to,system_key,project\n\
                 10000000-0000-4000-8000-000000000010,T10,member-a,,alpha\n"
        .to_owned();
    let invalid_habits = "task_id,assigned_to,system_key\nH1,member-a,\n";
    std::fs::write(tasks_dir.join("tasks.csv"), &tasks).unwrap();
    std::fs::write(tasks_dir.join("habits.csv"), invalid_habits).unwrap();
    std::fs::write(
        tasks_dir.join("SCHEMA.json"),
        r#"{"task_schema_version":2,"merge_key":"task_uuid","display_identity":{"field":"task_id","mutable":true}}"#,
    )
    .unwrap();
    let metadata = root.join("projects/alpha/.METADATA.json");
    std::fs::create_dir_all(metadata.parent().unwrap()).unwrap();
    let metadata_before = b"{\"name\":\"alpha\",\"tasks\":[\"T99\"]}\n";
    std::fs::write(&metadata, metadata_before).unwrap();
    let paths = paths(directory.path());
    let remote = BTreeMap::from([
        ("tasks/tasks.csv".to_owned(), tasks.clone()),
        ("tasks/habits.csv".to_owned(), invalid_habits.to_owned()),
    ]);
    let pushes = Cell::new(0);

    let result = sync_csvs_with_transport(
        &paths,
        &root,
        Direction::Both,
        |relative| remote.get(relative).cloned(),
        |_, _| {
            pushes.set(pushes.get() + 1);
            true
        },
    );

    assert!(matches!(result, Err(CsvSyncError::Preflight(_))));
    assert_eq!(
        std::fs::read_to_string(tasks_dir.join("tasks.csv")).unwrap(),
        tasks
    );
    assert_eq!(
        std::fs::read_to_string(tasks_dir.join("habits.csv")).unwrap(),
        invalid_habits
    );
    assert!(!baseline_path(&paths, "tasks.csv").exists());
    assert!(!baseline_path(&paths, "habits.csv").exists());
    assert_eq!(std::fs::read(&metadata).unwrap(), metadata_before);
    assert_eq!(pushes.get(), 0);
}

#[test]
fn python_legacy_writer_output_syncs_by_task_id_until_schema_activation() {
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("home");
    let root = directory.path().join("workspace");
    let tasks_dir = root.join("tasks");
    std::fs::create_dir_all(root.join(".config")).unwrap();
    std::fs::create_dir_all(&tasks_dir).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        root.join(".config/users.json"),
        r#"{"schema_version":1,"users":[{"id":"member-a","name":"Member A"}]}"#,
    )
    .unwrap();
    let legacy_tasks = "task_id,task_name,task_type,status,priority,assigned_to\n\
                        T1,Existing,personal,not_started,p2,member-a\n";
    let legacy_habits = "task_id,task_name,status,assigned_to\n";
    std::fs::write(tasks_dir.join("tasks.csv"), legacy_tasks).unwrap();
    std::fs::write(tasks_dir.join("habits.csv"), legacy_habits).unwrap();

    // A native create against a legacy-shaped table: the row it writes is what
    // the sync then has to keep task_id-keyed until the schema is activated.
    create_natively(&root, "New row");
    let hybrid = std::fs::read_to_string(tasks_dir.join("tasks.csv")).unwrap();
    assert!(hybrid.starts_with("task_id,"));
    assert!(hybrid.contains("task_uuid"));

    let remote = RefCell::new(BTreeMap::from([
        ("tasks/tasks.csv".to_owned(), legacy_tasks.to_owned()),
        ("tasks/habits.csv".to_owned(), legacy_habits.to_owned()),
    ]));
    let paths = paths(directory.path());
    let result = sync_csvs_with_transport(
        &paths,
        &root,
        Direction::Both,
        |relative| remote.borrow().get(relative).cloned(),
        |relative, text| {
            remote
                .borrow_mut()
                .insert(relative.to_owned(), text.to_owned());
            true
        },
    );

    assert!(result.is_ok(), "{result:?}");
    let local = std::fs::read_to_string(tasks_dir.join("tasks.csv")).unwrap();
    let table = parse(&local, crate::sync::csv_merge::SchemaStatus::Legacy).unwrap();
    assert_eq!(table.merge_key(), Some("task_id"));
    let name = table.column("task_name").unwrap();
    assert_eq!(table.rows["T1"][name], "Existing");
    assert_eq!(table.rows["T2"][name], "New row");
    assert_eq!(remote.borrow()["tasks/tasks.csv"], local);
}

#[test]
fn python_new_file_stays_task_id_keyed_until_schema_activation() {
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("home");
    let root = directory.path().join("workspace");
    std::fs::create_dir_all(root.join(".config")).unwrap();
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        root.join(".config/users.json"),
        r#"{"schema_version":1,"users":[{"id":"member-a","name":"Member A"}]}"#,
    )
    .unwrap();
    std::fs::write(root.join("tasks/tasks.csv"), "").unwrap();
    std::fs::write(root.join("tasks/habits.csv"), "task_id,status\n").unwrap();
    // A native create against a legacy-shaped table: the row it writes is what
    // the sync then has to keep task_id-keyed until the schema is activated.
    create_natively(&root, "First row");

    let remote = RefCell::new(BTreeMap::from([
        ("tasks/tasks.csv".to_owned(), String::new()),
        ("tasks/habits.csv".to_owned(), "task_id,status\n".to_owned()),
    ]));
    let paths = paths(directory.path());
    let result = sync_csvs_with_transport(
        &paths,
        &root,
        Direction::Both,
        |relative| remote.borrow().get(relative).cloned(),
        |relative, text| {
            remote
                .borrow_mut()
                .insert(relative.to_owned(), text.to_owned());
            true
        },
    );

    assert!(result.is_ok(), "{result:?}");
    let local = std::fs::read_to_string(root.join("tasks/tasks.csv")).unwrap();
    let table = parse(&local, crate::sync::csv_merge::SchemaStatus::Legacy).unwrap();
    assert_eq!(table.merge_key(), Some("task_id"));
    assert!(table.rows.contains_key("T1"));
    assert_eq!(remote.borrow()["tasks/tasks.csv"], local);
}

/// Create one ordinary task through the native writer.
///
/// These tests are about what the **sync** does with a freshly written row, so
/// the writer just has to be the real one.
fn create_natively(root: &std::path::Path, name: &str) {
    crate::tasks::add::create_in_root_for_actor_with_today(
        root,
        &crate::actor::test_actor("member-a"),
        &crate::tasks::add::CreateRequest {
            name: name.to_owned(),
            task_type: Some("personal".to_owned()),
            priority: "p2".to_owned(),
            ..crate::tasks::add::CreateRequest::default()
        },
        chrono::NaiveDate::from_ymd_opt(2026, 8, 24).expect("valid date"),
    )
    .expect("create the row");
}
