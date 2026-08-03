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
    _temporary: tempfile::TempDir,
    root: PathBuf,
    backup: PathBuf,
    original: BTreeMap<&'static str, Vec<u8>>,
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("workspace");
        let tasks_dir = root.join("tasks");
        let backup = temporary
            .path()
            .join("runtime/migration-backups/task-schema");
        std::fs::create_dir_all(&tasks_dir).unwrap();
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
            _temporary: temporary,
            root,
            backup,
            original,
        }
    }

    fn migrate(&self, sync: LegacySemanticSync) -> anyhow::Result<MigrationOutcome> {
        migrate_inactive(TaskSchemaMigration {
            workspace_id: WorkspaceId::parse(WORKSPACE_ID).unwrap(),
            workspace_root: &self.root,
            backup_dir: &self.backup,
            legacy_semantic_sync: sync,
        })
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

fn count_files(root: &Path) -> usize {
    walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .count()
}
