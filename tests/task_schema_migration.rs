include!("support/task_schema_migration_support.rs");

#[test]
fn deterministic_legacy_identity_is_scoped_by_workspace_and_csv_kind() {
    let workspace_id = WorkspaceId::parse(WORKSPACE_ID).unwrap();
    let other_workspace_id = WorkspaceId::parse(OTHER_WORKSPACE_ID).unwrap();

    let first = legacy_task_uuid(workspace_id, CsvKind::Tasks, "T42");
    let second = legacy_task_uuid(workspace_id, CsvKind::Tasks, "T42");

    assert_eq!(first, second);
    assert_eq!(
        uuid::Uuid::parse_str(&first.to_string())
            .unwrap()
            .get_version_num(),
        5
    );
    assert_ne!(
        first,
        legacy_task_uuid(other_workspace_id, CsvKind::Tasks, "T42")
    );
    assert_ne!(
        first,
        legacy_task_uuid(workspace_id, CsvKind::Habits, "T42")
    );
}

#[test]
fn a_mapped_legacy_assignment_moves_to_its_portable_member_in_both_csvs() {
    let unmapped = Fixture::new();
    unmapped.migrate(LegacySemanticSync::Complete).unwrap();
    assert_eq!(assignments(&unmapped), ["pablo", "wife", "wife"]);

    let mapped = Fixture::new();
    let mut rewrites = AssignmentRewrites::new();
    rewrites.record("wife", &brain::users::UserId::parse("sam").unwrap());

    migrate_inactive(TaskSchemaMigration {
        workspace_id: WorkspaceId::parse(WORKSPACE_ID).unwrap(),
        workspace_root: &mapped.root,
        task_store_lock: &mapped.temporary.path().join("tasks.transaction.lock"),
        preexisting_backup_base: &mapped.backup_base,
        backup_dir: &mapped.backup,
        legacy_semantic_sync: LegacySemanticSync::Complete,
        assignment_rewrites: &rewrites,
    })
    .unwrap();

    assert_eq!(assignments(&mapped), ["pablo", "sam", "sam"]);
    assert_eq!(
        std::fs::read(mapped.backup.join("tasks/tasks.csv")).unwrap(),
        mapped.original["tasks.csv"],
        "the retained backup keeps the pre-rewrite assignments"
    );
}

fn assignments(fixture: &Fixture) -> Vec<String> {
    ["tasks.csv", "habits.csv"]
        .into_iter()
        .flat_map(|name| {
            let (_, rows) = fixture.csv(name);
            rows.into_iter()
                .map(|row| row["assigned_to"].clone())
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn inactive_migration_requires_the_rollout_owned_legacy_sync_precondition() {
    let fixture = Fixture::new();

    let error = fixture.migrate(LegacySemanticSync::Required).unwrap_err();

    assert!(error.to_string().contains("legacy semantic sync"));
    for (name, bytes) in &fixture.original {
        assert_eq!(
            std::fs::read(fixture.root.join("tasks").join(name)).unwrap(),
            *bytes
        );
    }
    assert!(!fixture.backup.exists());
}

#[test]
fn inactive_migration_requires_an_existing_durable_backup_base() {
    let fixture = Fixture::new();
    let missing_base = fixture.temporary.path().join("missing-backup-base");
    let backup = missing_base.join("deep/task-schema");

    let error = migrate_inactive(TaskSchemaMigration {
        workspace_id: WorkspaceId::parse(WORKSPACE_ID).unwrap(),
        workspace_root: &fixture.root,
        task_store_lock: &fixture.temporary.path().join("tasks.transaction.lock"),
        preexisting_backup_base: &missing_base,
        backup_dir: &backup,
        legacy_semantic_sync: LegacySemanticSync::Complete,
        assignment_rewrites: &AssignmentRewrites::new(),
    })
    .unwrap_err();

    assert!(error.to_string().contains("backup base must already exist"));
    fixture.assert_live_inputs_unchanged();
    assert!(!backup.exists());
}

#[test]
fn inactive_migration_refuses_a_newer_task_schema_without_changing_live_files() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.root.join("tasks/SCHEMA.json"),
        br#"{"task_schema_version":3,"merge_key":"task_uuid","display_identity":{"field":"task_id","mutable":true}}"#,
    )
    .unwrap();
    let before = ["tasks.csv", "habits.csv", "SCHEMA.json"].map(|name| {
        (
            name,
            std::fs::read(fixture.root.join("tasks").join(name)).unwrap(),
        )
    });

    let error = fixture
        .migrate(LegacySemanticSync::Complete)
        .expect_err("a future schema must not be downgraded");

    assert!(error.to_string().contains("task schema 3"), "{error:#}");
    for (name, bytes) in before {
        assert_eq!(
            std::fs::read(fixture.root.join("tasks").join(name)).unwrap(),
            bytes
        );
    }
    assert!(!fixture.backup.exists());
}

#[test]
fn fixture_migration_preserves_rows_and_display_ids_and_backs_up_portable_inputs() {
    let fixture = Fixture::new();

    assert_eq!(
        fixture.migrate(LegacySemanticSync::Complete).unwrap(),
        MigrationOutcome::Migrated
    );

    let (task_header, task_rows) = fixture.csv("tasks.csv");
    let (habit_header, habit_rows) = fixture.csv("habits.csv");
    assert_eq!(task_header[0], "task_uuid");
    assert_eq!(habit_header[0], "task_uuid");
    assert_eq!(task_rows.len(), 2);
    assert_eq!(habit_rows.len(), 1);
    assert_eq!(task_rows[0]["task_id"], "T42");
    assert_eq!(task_rows[1]["task_id"], "T7");
    assert_eq!(habit_rows[0]["task_id"], "H3");
    assert_eq!(task_rows[0]["assigned_to"], "pablo");
    assert!(!task_header.iter().any(|column| column == "assignee"));
    assert_eq!(
        task_rows[0]["task_uuid"],
        legacy_task_uuid(
            WorkspaceId::parse(WORKSPACE_ID).unwrap(),
            CsvKind::Tasks,
            "T42"
        )
        .to_string()
    );
    assert_eq!(habit_rows[0]["system_key"], "brain.triage.daily");

    for (name, bytes) in &fixture.original {
        assert_eq!(
            std::fs::read(fixture.backup.join("tasks").join(name)).unwrap(),
            *bytes,
            "backup for {name} must retain exact legacy bytes"
        );
    }
    let schema: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture.root.join("tasks/SCHEMA.json")).unwrap())
            .unwrap();
    assert_eq!(schema["task_schema_version"], 2);
    assert_eq!(schema["merge_key"], "task_uuid");
    assert_eq!(schema["display_identity"]["field"], "task_id");
    assert_eq!(schema["display_identity"]["mutable"], true);
    assert_eq!(schema["labels"]["priority"], "Priority");
}

#[test]
fn fixture_migration_is_byte_idempotent() {
    let fixture = Fixture::new();
    fixture.migrate(LegacySemanticSync::Complete).unwrap();
    let first_tasks = std::fs::read(fixture.root.join("tasks/tasks.csv")).unwrap();
    let first_habits = std::fs::read(fixture.root.join("tasks/habits.csv")).unwrap();
    let first_schema = std::fs::read(fixture.root.join("tasks/SCHEMA.json")).unwrap();
    let backup_entries = count_files(&fixture.backup);

    assert_eq!(
        fixture.migrate(LegacySemanticSync::Complete).unwrap(),
        MigrationOutcome::AlreadyCurrent
    );

    assert_eq!(
        std::fs::read(fixture.root.join("tasks/tasks.csv")).unwrap(),
        first_tasks
    );
    assert_eq!(
        std::fs::read(fixture.root.join("tasks/habits.csv")).unwrap(),
        first_habits
    );
    assert_eq!(
        std::fs::read(fixture.root.join("tasks/SCHEMA.json")).unwrap(),
        first_schema
    );
    assert_eq!(count_files(&fixture.backup), backup_entries);
}

#[test]
fn backup_destination_must_be_disjoint_from_the_workspace_tree() {
    for label in ["equal", "nested", "tasks", "lexical tasks", "ancestor"] {
        let fixture = Fixture::new();
        let backup = match label {
            "equal" => fixture.root.clone(),
            "nested" => fixture.root.join("runtime/task-schema-backup"),
            "tasks" => fixture.root.join("tasks"),
            "lexical tasks" => fixture.root.join("runtime/../tasks/task-schema-backup"),
            "ancestor" => fixture.temporary.path().to_path_buf(),
            _ => unreachable!(),
        };

        let error = fixture
            .migrate_to(LegacySemanticSync::Complete, &backup)
            .unwrap_err();

        assert!(
            error.to_string().contains("disjoint"),
            "{label} overlap should be rejected: {error:#}"
        );
        fixture.assert_live_inputs_unchanged();
    }
}

#[test]
fn disjoint_machine_local_backup_path_is_accepted() {
    let fixture = Fixture::new();
    let backup = fixture.temporary.path().join("machine-local/task-schema");

    assert_eq!(
        fixture
            .migrate_to(LegacySemanticSync::Complete, &backup)
            .unwrap(),
        MigrationOutcome::Migrated
    );
    assert!(backup.join("tasks/tasks.csv").is_file());
}

#[cfg(unix)]
#[test]
fn backup_destination_resolves_symlink_aliases_before_overlap_checking() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    let alias = fixture.temporary.path().join("workspace-alias");
    symlink(&fixture.root, &alias).unwrap();

    let error = fixture
        .migrate_to(
            LegacySemanticSync::Complete,
            &alias.join("task-schema-backup"),
        )
        .unwrap_err();

    assert!(error.to_string().contains("disjoint"));
    fixture.assert_live_inputs_unchanged();
}

#[test]
fn current_detection_requires_complete_identity_metadata_and_columns() {
    let cases = [
        (
            "merge key",
            "task_uuid,task_id,task_name,assigned_to,system_key",
            "task_uuid,task_id,task_name,assigned_to,system_key",
            r#"{"task_schema_version":2,"merge_key":"task_id","display_identity":{"field":"task_id","mutable":true}}"#,
        ),
        (
            "display field",
            "task_uuid,task_id,task_name,assigned_to,system_key",
            "task_uuid,task_id,task_name,assigned_to,system_key",
            r#"{"task_schema_version":2,"merge_key":"task_uuid","display_identity":{"field":"task_uuid","mutable":true}}"#,
        ),
        (
            "display mutability",
            "task_uuid,task_id,task_name,assigned_to,system_key",
            "task_uuid,task_id,task_name,assigned_to,system_key",
            r#"{"task_schema_version":2,"merge_key":"task_uuid","display_identity":{"field":"task_id","mutable":false}}"#,
        ),
        (
            "task system key",
            "task_uuid,task_id,task_name,assigned_to,notes",
            "task_uuid,task_id,task_name,assigned_to,system_key",
            r#"{"task_schema_version":2,"merge_key":"task_uuid","display_identity":{"field":"task_id","mutable":true}}"#,
        ),
        (
            "habit system key",
            "task_uuid,task_id,task_name,assigned_to,system_key",
            "task_uuid,task_id,task_name,assigned_to,notes",
            r#"{"task_schema_version":2,"merge_key":"task_uuid","display_identity":{"field":"task_id","mutable":true}}"#,
        ),
    ];

    for (label, tasks_header, habits_header, schema) in cases {
        let fixture = Fixture::new();
        fixture.write_current_like(tasks_header, habits_header, schema);

        assert_eq!(
            fixture.migrate(LegacySemanticSync::Complete).unwrap(),
            MigrationOutcome::Migrated,
            "incomplete {label} must not count as current"
        );

        let (task_header, _) = fixture.csv("tasks.csv");
        let (habit_header, _) = fixture.csv("habits.csv");
        assert!(task_header.iter().any(|column| column == "system_key"));
        assert!(habit_header.iter().any(|column| column == "system_key"));
        let metadata: serde_json::Value =
            serde_json::from_slice(&std::fs::read(fixture.root.join("tasks/SCHEMA.json")).unwrap())
                .unwrap();
        assert_eq!(metadata["merge_key"], "task_uuid");
        assert_eq!(metadata["display_identity"]["field"], "task_id");
        assert_eq!(metadata["display_identity"]["mutable"], true);
    }
}

fn count_files(root: &Path) -> usize {
    walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .count()
}
