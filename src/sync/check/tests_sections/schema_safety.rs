
#[test]
fn active_schema_v2_diff_keys_distinct_uuids_with_one_display_id() {
    let dir = tempfile::tempdir().unwrap();
    let tasks = dir.path().join("tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    std::fs::write(
        tasks.join("SCHEMA.json"),
        r#"{"task_schema_version":2,"merge_key":"task_uuid"}"#,
    )
    .unwrap();
    let base = "task_uuid,task_id,assigned_to,system_key\n\
                    10000000-0000-4000-8000-000000000001,T1,member-a,\n";
    let local = "task_uuid,task_id,assigned_to,system_key\n\
                     20000000-0000-4000-8000-000000000001,T1,member-a,\n";
    std::fs::write(tasks.join("tasks.csv"), local).unwrap();

    let pending = collect_csv_pending_with_fetch(
        dir.path(),
        &["tasks/tasks.csv"],
        |_| Ok(base.to_owned()),
        |_| Some(base.to_owned()),
    )
    .unwrap();

    assert_eq!(pending[0].push.added, 1);
    assert_eq!(pending[0].push.deleted, 1);
    assert_eq!(pending[0].push.changed, 0);
}

#[test]
fn inactive_schema_hybrid_diff_remains_task_id_keyed() {
    let dir = tempfile::tempdir().unwrap();
    let tasks = dir.path().join("tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    let base = "task_id,task_uuid,status\nT1,,not_started\n";
    let local = "task_id,task_uuid,status\n\
                     T1,,done\n\
                     T2,20000000-0000-4000-8000-000000000002,not_started\n";
    std::fs::write(tasks.join("tasks.csv"), local).unwrap();

    let pending = collect_csv_pending_with_fetch(
        dir.path(),
        &["tasks/tasks.csv"],
        |_| Ok(base.to_owned()),
        |_| Some(base.to_owned()),
    )
    .unwrap();

    assert_eq!(pending[0].push.added, 1);
    assert_eq!(pending[0].push.changed, 1);
    assert_eq!(pending[0].push.deleted, 0);
}

#[test]
fn invalid_check_generations_are_labeled_and_leave_every_store_unchanged() {
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    for (generation, baseline, local, remote) in [
        (
            "baseline",
            "task_id,status\nT1,open\nT1,done\n",
            "task_id,status\nT1,open\n",
            "task_id,status\nT1,open\n",
        ),
        (
            "local",
            "task_id,status\nT1,open\n",
            "task_id,status\nT1,open,extra\n",
            "task_id,status\nT1,open\n",
        ),
        (
            "remote",
            "task_id,status\nT1,open\n",
            "task_id,status\nT1,open\n",
            "task_id,status\nT1,open\nT1,done\n",
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let tasks = dir.path().join("tasks");
        let project = dir.path().join("projects/alpha");
        std::fs::create_dir_all(&tasks).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(tasks.join("tasks.csv"), local).unwrap();
        std::fs::write(tasks.join("habits.csv"), "task_id,status\nH1,open\n").unwrap();
        std::fs::write(tasks.join(".tasks_next_id"), "2\n").unwrap();
        std::fs::write(tasks.join(".habits_next_id"), "2\n").unwrap();
        std::fs::write(project.join(".METADATA.json"), b"{\"name\":\"alpha\"}\n").unwrap();
        let baseline_path = dir.path().join("cache/baselines/tasks.csv");
        std::fs::create_dir_all(baseline_path.parent().unwrap()).unwrap();
        std::fs::write(&baseline_path, baseline).unwrap();
        let remote_store = RefCell::new(BTreeMap::from([(
            "tasks/tasks.csv".to_owned(),
            remote.to_owned(),
        )]));
        let snapshots = [
            (
                tasks.join("tasks.csv"),
                std::fs::read(tasks.join("tasks.csv")).unwrap(),
            ),
            (
                tasks.join("habits.csv"),
                std::fs::read(tasks.join("habits.csv")).unwrap(),
            ),
            (
                tasks.join(".tasks_next_id"),
                std::fs::read(tasks.join(".tasks_next_id")).unwrap(),
            ),
            (
                tasks.join(".habits_next_id"),
                std::fs::read(tasks.join(".habits_next_id")).unwrap(),
            ),
            (
                project.join(".METADATA.json"),
                std::fs::read(project.join(".METADATA.json")).unwrap(),
            ),
            (
                baseline_path.clone(),
                std::fs::read(&baseline_path).unwrap(),
            ),
        ];
        let remote_before = remote_store.borrow().clone();

        let error = collect_csv_pending_with_fetch(
            dir.path(),
            &["tasks/tasks.csv"],
            |_| std::fs::read_to_string(&baseline_path).map_err(|error| error.to_string()),
            |relative| remote_store.borrow().get(relative).cloned(),
        )
        .unwrap_err();

        let message = error.to_string();
        assert!(message.contains(generation), "{message}");
        assert!(message.contains("tasks/tasks.csv"), "{message}");
        for (path, before) in snapshots {
            assert_eq!(std::fs::read(path).unwrap(), before);
        }
        assert_eq!(*remote_store.borrow(), remote_before);
    }
}

#[test]
fn invalid_manifest_and_csv_render_warning_without_false_clean_claim() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
    std::fs::write(dir.path().join("tasks/SCHEMA.json"), "not json\n").unwrap();
    std::fs::write(
        dir.path().join("tasks/tasks.csv"),
        "task_id,status\nT1,open\n",
    )
    .unwrap();

    let error = collect_csv_pending_with_fetch(
        dir.path(),
        &["tasks/tasks.csv"],
        |_| Ok("task_id,status\nT1,open\n".to_owned()),
        |_| Some("task_id,status\nT1,open\n".to_owned()),
    )
    .unwrap_err();
    let warning = format_csv_check_error(&error, Theme::dark(false));

    assert!(warning.contains("Could not check task and habit CSV changes"));
    assert!(warning.contains("tasks/SCHEMA.json"));
    assert!(!warning.contains("In sync"));
}
