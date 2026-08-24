
#[test]
fn malformed_or_duplicate_generation_refuses_the_whole_operation() {
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    struct Case {
        name: &'static str,
        manifest: Option<&'static str>,
        local_tasks: &'static str,
        remote_tasks: &'static str,
        expected: &'static str,
    }
    let cases = [
        Case {
            name: "duplicate local current UUID",
            manifest: Some(
                r#"{"task_schema_version":2,"merge_key":"task_uuid","display_identity":{"field":"task_id","mutable":true}}"#,
            ),
            local_tasks: "task_uuid,task_id,assigned_to,system_key\n\
                          10000000-0000-4000-8000-000000000001,T1,member-a,\n\
                          10000000-0000-4000-8000-000000000001,T2,member-a,\n",
            remote_tasks: "task_uuid,task_id,assigned_to,system_key\n",
            expected: "local tasks/tasks.csv: duplicate task_uuid",
        },
        Case {
            name: "duplicate remote legacy display ID",
            manifest: None,
            local_tasks: "task_id,status\nT1,not_started\n",
            remote_tasks: "task_id,status\nT1,not_started\nT1,done\n",
            expected: "remote tasks/tasks.csv: duplicate task_id",
        },
        Case {
            name: "malformed remote record",
            manifest: None,
            local_tasks: "task_id,notes\nT1,ok\n",
            remote_tasks: "task_id,notes\nT1,ok\nT2,ok,unexpected\n",
            expected: "remote tasks/tasks.csv: malformed CSV record",
        },
    ];

    for case in cases {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("workspace");
        let tasks_dir = root.join("tasks");
        std::fs::create_dir_all(root.join("projects/alpha")).unwrap();
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(tasks_dir.join("tasks.csv"), case.local_tasks).unwrap();
        let habits = if case.manifest.is_some() {
            "task_uuid,task_id,assigned_to,system_key\n"
        } else {
            "task_id,status\n"
        };
        std::fs::write(tasks_dir.join("habits.csv"), habits).unwrap();
        std::fs::write(tasks_dir.join(".tasks_next_id"), "9\n").unwrap();
        std::fs::write(tasks_dir.join(".habits_next_id"), "4\n").unwrap();
        if let Some(manifest) = case.manifest {
            std::fs::write(tasks_dir.join("SCHEMA.json"), manifest).unwrap();
        }
        let metadata = root.join("projects/alpha/.METADATA.json");
        std::fs::write(&metadata, b"{\"name\":\"alpha\",\"tasks\":[\"T9\"]}\n").unwrap();
        let paths = paths(directory.path());
        std::fs::create_dir_all(paths.sync_csv_baselines()).unwrap();
        let valid_baseline = if case.manifest.is_some() {
            "task_uuid,task_id,assigned_to,system_key\n"
        } else {
            "task_id,status\n"
        };
        std::fs::write(baseline_path(&paths, "tasks.csv"), valid_baseline).unwrap();
        std::fs::write(baseline_path(&paths, "habits.csv"), habits).unwrap();
        let remote = RefCell::new(BTreeMap::from([
            ("tasks/tasks.csv".to_owned(), case.remote_tasks.to_owned()),
            ("tasks/habits.csv".to_owned(), habits.to_owned()),
            ("tasks/.tasks_next_id".to_owned(), "12\n".to_owned()),
            ("tasks/.habits_next_id".to_owned(), "7\n".to_owned()),
        ]));
        if let Some(manifest) = case.manifest {
            remote
                .borrow_mut()
                .insert("tasks/SCHEMA.json".to_owned(), manifest.to_owned());
        }
        let before_remote = remote.borrow().clone();
        let before_local_tasks = std::fs::read(tasks_dir.join("tasks.csv")).unwrap();
        let before_local_habits = std::fs::read(tasks_dir.join("habits.csv")).unwrap();
        let before_task_baseline = std::fs::read(baseline_path(&paths, "tasks.csv")).unwrap();
        let before_habit_baseline = std::fs::read(baseline_path(&paths, "habits.csv")).unwrap();
        let before_metadata = std::fs::read(&metadata).unwrap();
        let before_task_counter = std::fs::read(tasks_dir.join(".tasks_next_id")).unwrap();
        let before_habit_counter = std::fs::read(tasks_dir.join(".habits_next_id")).unwrap();

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

        let error = result.expect_err(case.name).to_string();
        assert!(
            error.contains(case.expected),
            "{case_name}: {error}",
            case_name = case.name
        );
        assert_eq!(
            std::fs::read(tasks_dir.join("tasks.csv")).unwrap(),
            before_local_tasks
        );
        assert_eq!(
            std::fs::read(tasks_dir.join("habits.csv")).unwrap(),
            before_local_habits
        );
        assert_eq!(
            std::fs::read(baseline_path(&paths, "tasks.csv")).unwrap(),
            before_task_baseline
        );
        assert_eq!(
            std::fs::read(baseline_path(&paths, "habits.csv")).unwrap(),
            before_habit_baseline
        );
        assert_eq!(std::fs::read(&metadata).unwrap(), before_metadata);
        assert_eq!(
            std::fs::read(tasks_dir.join(".tasks_next_id")).unwrap(),
            before_task_counter
        );
        assert_eq!(
            std::fs::read(tasks_dir.join(".habits_next_id")).unwrap(),
            before_habit_counter
        );
        assert_eq!(*remote.borrow(), before_remote);
    }
}

#[test]
fn push_only_collision_floors_task_and_habit_counters_before_allocation() {
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("workspace");
    let tasks_dir = root.join("tasks");
    std::fs::create_dir_all(&tasks_dir).unwrap();
    let manifest = r#"{"task_schema_version":2,"merge_key":"task_uuid","display_identity":{"field":"task_id","mutable":true}}"#;
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
        ("tasks/SCHEMA.json".to_owned(), manifest.to_owned()),
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

    // The next id a native create allocates is the counter's answer: the
    // floors the push wrote must be what a later row starts from.
    let actor = crate::actor::test_actor("member-a");
    let allocate = |habit: bool| {
        crate::tasks::add::create_in_root_for_actor_with_today(
            &root,
            &actor,
            &crate::tasks::add::CreateRequest {
                name: "Next".to_owned(),
                priority: "p2".to_owned(),
                task_type: (!habit).then(|| "personal".to_owned()),
                habit,
                interval: habit.then_some(1),
                unit: habit.then(|| "days".to_owned()),
                due: habit.then(|| "2026-08-24".to_owned()),
                ..crate::tasks::add::CreateRequest::default()
            },
            chrono::NaiveDate::from_ymd_opt(2026, 8, 24).expect("valid date"),
        )
        .expect("allocate")
        .created[0]
            .id
            .clone()
    };

    assert_eq!(allocate(false), "T12");
    assert_eq!(allocate(true), "H7");
}
