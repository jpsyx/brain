use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use brain::tasks::identity::{CsvKind, legacy_task_uuid};
use brain::tasks::schema::{
    LegacySemanticSync, MigrationOutcome, TaskSchemaMigration, migrate_inactive,
};
use brain::workspace::WorkspaceId;

const WORKSPACE_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";
const OTHER_WORKSPACE_ID: &str = "e806258e-491a-436d-9db4-a5ca9903e0d4";

struct Fixture {
    temporary: tempfile::TempDir,
    root: PathBuf,
    backup_base: PathBuf,
    backup: PathBuf,
    original: BTreeMap<&'static str, Vec<u8>>,
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("workspace");
        let tasks_dir = root.join("tasks");
        let backup_base = temporary.path().join("machine-local");
        let backup = backup_base.join("runtime/migration-backups/task-schema");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::create_dir(&backup_base).unwrap();
        let files = [
            (
                "tasks.csv",
                b"task_name,task_id,status,assignee,notes\nFirst,T42,not_started,pablo,keep\nSecond,T7,done,wife,also keep\n".as_slice(),
            ),
            (
                "habits.csv",
                b"task_id,task_name,status,assigned_to,system_key,due_date,recur_interval,recur_unit\nH3,Morning triage,not_started,wife,brain.triage.daily,2026-08-03,1,days\n".as_slice(),
            ),
            (".tasks_next_id", b"43\n".as_slice()),
            (".habits_next_id", b"4\n".as_slice()),
            (
                "SCHEMA.json",
                b"{\n  \"labels\": {\"priority\": \"Priority\"}\n}\n".as_slice(),
            ),
        ];
        let mut original = BTreeMap::new();
        for (name, bytes) in files {
            std::fs::write(tasks_dir.join(name), bytes).unwrap();
            original.insert(name, bytes.to_vec());
        }
        Self {
            temporary,
            root,
            backup_base,
            backup,
            original,
        }
    }

    fn migrate(&self, sync: LegacySemanticSync) -> anyhow::Result<MigrationOutcome> {
        self.migrate_to(sync, &self.backup)
    }

    fn migrate_to(
        &self,
        sync: LegacySemanticSync,
        backup_dir: &Path,
    ) -> anyhow::Result<MigrationOutcome> {
        migrate_inactive(TaskSchemaMigration {
            workspace_id: WorkspaceId::parse(WORKSPACE_ID).unwrap(),
            workspace_root: &self.root,
            preexisting_backup_base: &self.backup_base,
            backup_dir,
            legacy_semantic_sync: sync,
        })
    }

    fn assert_live_inputs_unchanged(&self) {
        for (name, bytes) in &self.original {
            assert_eq!(
                std::fs::read(self.root.join("tasks").join(name)).unwrap(),
                *bytes,
                "live input {name} changed"
            );
        }
    }

    fn write_current_like(&self, tasks_header: &str, habits_header: &str, schema: &str) {
        std::fs::write(
            self.root.join("tasks/tasks.csv"),
            format!("{tasks_header}\n8f4ff482-4d40-4a2d-91b1-73ca9f1bfad4,T42,First,pablo,\n"),
        )
        .unwrap();
        std::fs::write(
            self.root.join("tasks/habits.csv"),
            format!(
                "{habits_header}\n647a98fc-978b-4ab5-97f4-f291b56747d7,H3,Morning triage,pablo,brain.triage.daily\n"
            ),
        )
        .unwrap();
        std::fs::write(self.root.join("tasks/SCHEMA.json"), schema).unwrap();
    }

    fn csv(&self, name: &str) -> (Vec<String>, Vec<BTreeMap<String, String>>) {
        let mut reader = csv::Reader::from_path(self.root.join("tasks").join(name)).unwrap();
        let headers = reader
            .headers()
            .unwrap()
            .iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let rows = reader
            .records()
            .map(|record| {
                let record = record.unwrap();
                headers
                    .iter()
                    .zip(record.iter())
                    .map(|(column, value)| (column.clone(), value.to_owned()))
                    .collect()
            })
            .collect();
        (headers, rows)
    }
}

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
        preexisting_backup_base: &missing_base,
        backup_dir: &backup,
        legacy_semantic_sync: LegacySemanticSync::Complete,
    })
    .unwrap_err();

    assert!(error.to_string().contains("backup base must already exist"));
    fixture.assert_live_inputs_unchanged();
    assert!(!backup.exists());
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
