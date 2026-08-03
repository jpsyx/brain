use super::*;

fn paths(home: &Path) -> crate::workspace::WorkspacePaths {
    crate::workspace::WorkspacePaths::new(home, crate::workspace::WorkspaceId::new())
}

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

#[test]
fn reconciled_project_metadata_is_written_and_pushed_with_final_ids() {
    use std::cell::RefCell;

    let directory = tempfile::tempdir().unwrap();
    let metadata = directory.path().join("projects/alpha/.METADATA.json");
    std::fs::create_dir_all(metadata.parent().unwrap()).unwrap();
    std::fs::write(
        &metadata,
        b"{\"name\":\"alpha\",\"title\":\"Alpha\",\"tasks\":[\"T10\"]}\n",
    )
    .unwrap();
    let table = parse(
        "task_uuid,task_id,project\n\
             10000000-0000-4000-8000-000000000010,T10,alpha\n\
             20000000-0000-4000-8000-000000000010,T13,alpha\n",
    );
    let pushed = RefCell::new(Vec::new());

    let changed = reconcile_project_metadata(directory.path(), &[table], true, |relative, text| {
        pushed
            .borrow_mut()
            .push((relative.to_owned(), text.to_owned()));
        true
    })
    .unwrap();

    let local: serde_json::Value =
        serde_json::from_slice(&std::fs::read(metadata).unwrap()).unwrap();
    assert_eq!(changed, 1);
    assert_eq!(local["title"], "Alpha");
    assert_eq!(local["tasks"], serde_json::json!(["T10", "T13"]));
    assert_eq!(pushed.borrow().len(), 1);
    assert_eq!(pushed.borrow()[0].0, "projects/alpha/.METADATA.json");
}

#[test]
fn malformed_project_metadata_aborts_before_rewriting_unrelated_projects() {
    let directory = tempfile::tempdir().unwrap();
    let alpha = directory.path().join("projects/alpha/.METADATA.json");
    let broken = directory.path().join("projects/zeta/.METADATA.json");
    std::fs::create_dir_all(alpha.parent().unwrap()).unwrap();
    std::fs::create_dir_all(broken.parent().unwrap()).unwrap();
    let original = b"{\"name\":\"alpha\",\"tasks\":[\"T10\"]}\n";
    std::fs::write(&alpha, original).unwrap();
    std::fs::write(&broken, b"not json\n").unwrap();
    let table = parse(
        "task_uuid,task_id,project\n\
             10000000-0000-4000-8000-000000000010,T13,alpha\n",
    );

    let result = reconcile_project_metadata(directory.path(), &[table], true, |_, _| true);

    assert!(result.is_err());
    assert_eq!(std::fs::read(alpha).unwrap(), original);
}

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
        r#"{"task_schema_version":2,"merge_key":"task_uuid"}"#,
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
fn push_only_collision_floors_task_and_habit_counters_before_allocation() {
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::process::Command;

    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("workspace");
    let tasks_dir = root.join("tasks");
    std::fs::create_dir_all(&tasks_dir).unwrap();
    let manifest = r#"{"task_schema_version":2,"merge_key":"task_uuid"}"#;
    std::fs::write(tasks_dir.join("SCHEMA.json"), manifest).unwrap();
    let task_header = "task_uuid,task_id,assigned_to,system_key\n";
    let habit_header = "task_uuid,task_id,assigned_to,system_key\n";
    std::fs::write(
        tasks_dir.join("tasks.csv"),
        format!("{task_header}10000000-0000-4000-8000-000000000010,T10,member-a,\n"),
    )
    .unwrap();
    std::fs::write(
        tasks_dir.join("habits.csv"),
        format!("{habit_header}30000000-0000-4000-8000-000000000005,H5,member-a,\n"),
    )
    .unwrap();
    std::fs::write(tasks_dir.join(".tasks_next_id"), "11\n").unwrap();
    std::fs::write(tasks_dir.join(".habits_next_id"), "6\n").unwrap();
    let remote = RefCell::new(BTreeMap::from([
        (
            "tasks/tasks.csv".to_owned(),
            format!("{task_header}20000000-0000-4000-8000-000000000010,T10,member-a,\n"),
        ),
        (
            "tasks/habits.csv".to_owned(),
            format!("{habit_header}40000000-0000-4000-8000-000000000005,H5,member-a,\n"),
        ),
        ("tasks/.tasks_next_id".to_owned(), "11\n".to_owned()),
        ("tasks/.habits_next_id".to_owned(), "6\n".to_owned()),
    ]));
    let paths = paths(directory.path());

    let csv = sync_csvs_with_transport(
        &paths,
        &root,
        Direction::Push,
        |relative| remote.borrow().get(relative).cloned(),
        |relative, text| {
            remote
                .borrow_mut()
                .insert(relative.to_owned(), text.to_owned());
            true
        },
    )
    .unwrap();
    let _ = crate::sync::counters::sync_counters_with_transport(
        &root,
        Direction::Push,
        csv.floors,
        |relative| remote.borrow().get(relative).cloned(),
        |relative, text| {
            remote
                .borrow_mut()
                .insert(relative.to_owned(), text.to_owned());
            true
        },
    );

    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("skills/todo/scripts/next_id.py");
    let allocate = |kind: &str| {
        let output = Command::new("python3")
            .arg(&script)
            .args(["--kind", kind])
            .current_dir(script.parent().unwrap())
            .env("BRAIN_ROOT", &root)
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };

    assert_eq!(allocate("tasks"), "T12");
    assert_eq!(allocate("habits"), "H7");
}
