#[test]
fn baseline_path_is_under_cache_brain_sync_baselines() {
    let paths = paths(Path::new("/home/tester"));
    assert!(baseline_path(&paths, "tasks.csv").ends_with("sync/baselines/tasks.csv"));
    assert!(baseline_path(&paths, "habits.csv").ends_with("sync/baselines/habits.csv"));
}

#[test]
fn remote_csv_arg_joins_and_trims_a_trailing_slash() {
    assert_eq!(
        remote_csv_arg("BRAIN:bucket/pre", "tasks/tasks.csv"),
        "BRAIN:bucket/pre/tasks/tasks.csv"
    );
    assert_eq!(
        remote_csv_arg("BRAIN:bucket/pre/", "tasks/habits.csv"),
        "BRAIN:bucket/pre/tasks/habits.csv"
    );
}

#[test]
fn push_only_merge_preserves_remote_rows_without_downloading_them() {
    use std::cell::RefCell;

    let base = std::env::temp_dir().join(format!("brain-csv-push-only-{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    let local = base.join("local.csv");
    let rel = format!("tasks/push-only-{}.csv", std::process::id());
    let name = Path::new(&rel).file_name().unwrap().to_str().unwrap();
    let paths = paths(&base);
    let baseline = baseline_path(&paths, name);
    std::fs::remove_file(&baseline).ok();
    let header = "task_id,status,notes,last_touched\n";
    std::fs::write(&local, format!("{header}A,open,local,t1\n")).unwrap();
    let uploaded = RefCell::new(String::new());

    sync_one_push_only(
        &paths,
        &local,
        &rel,
        || Some(format!("{header}B,open,remote,t1\n")),
        |text| {
            uploaded.replace(text.to_owned());
            true
        },
    );

    let local_after = std::fs::read_to_string(&local).unwrap();
    assert!(local_after.contains("A,open,local"));
    assert!(!local_after.contains("B,open,remote"));
    let remote_after = uploaded.borrow();
    assert!(remote_after.contains("A,open,local"));
    assert!(remote_after.contains("B,open,remote"));
    assert!(
        !baseline.exists(),
        "push-only must not advance the downstream baseline"
    );

    std::fs::remove_dir_all(base).ok();
}

#[test]
fn unsupported_current_schema_refuses_all_csv_writes() {
    use std::cell::Cell;

    let directory = tempfile::tempdir().unwrap();
    let tasks = directory.path().join("workspace/tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    let local = tasks.join("tasks.csv");
    let text = "task_uuid,task_id,assigned_to,system_key,last_touched\n\
                    10000000-0000-4000-8000-000000000010,T10,member-a,,2026-08-02\n";
    std::fs::write(&local, text).unwrap();
    std::fs::write(
        tasks.join("SCHEMA.json"),
        r#"{"task_schema_version":3,"merge_key":"task_uuid"}"#,
    )
    .unwrap();
    let paths = paths(directory.path());
    let pushed = Cell::new(false);

    sync_one(
        &paths,
        &local,
        "tasks/tasks.csv",
        || Some(text.to_owned()),
        |_| {
            pushed.set(true);
            true
        },
    );

    assert_eq!(std::fs::read_to_string(&local).unwrap(), text);
    assert!(!pushed.get());
    assert!(!baseline_path(&paths, "tasks.csv").exists());
}
