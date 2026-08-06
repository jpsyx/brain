use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use brain::tasks::identity::{CsvKind, legacy_task_uuid};
use brain::tasks::schema::{
    LegacySemanticSync, MigrationOutcome, TaskSchemaMigration, migrate_inactive,
};
use brain::users::AssignmentRewrites;
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
            task_store_lock: &self.temporary.path().join("tasks.transaction.lock"),
            preexisting_backup_base: &self.backup_base,
            backup_dir,
            legacy_semantic_sync: sync,
            assignment_rewrites: &AssignmentRewrites::new(),
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
