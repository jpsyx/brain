use std::cell::Cell;
use std::collections::BTreeMap;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

use super::*;
use crate::tasks::identity::legacy_task_uuid;

struct Fixture {
    _temporary: tempfile::TempDir,
    root: PathBuf,
    backup_base: PathBuf,
    backup: PathBuf,
    task_store_lock: PathBuf,
    original: BTreeMap<&'static str, Vec<u8>>,
    rewrites: crate::users::AssignmentRewrites,
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("workspace");
        let tasks = root.join("tasks");
        let backup_base = temporary.path().join("machine-local");
        let backup = backup_base.join("level-one/level-two/task-schema");
        fs::create_dir_all(&tasks).unwrap();
        fs::create_dir(&backup_base).unwrap();
        let files = [
                (
                    "tasks.csv",
                    b"task_id,task_name,status,assignee\nT42,First,not_started,pablo\nT7,Second,done,wife\n"
                        .as_slice(),
                ),
                (
                    "habits.csv",
                    b"task_id,task_name,status,assigned_to,system_key\nH3,Triage,not_started,pablo,brain.triage.daily\n"
                        .as_slice(),
                ),
                (".tasks_next_id", b"43\n".as_slice()),
                (".habits_next_id", b"4\n".as_slice()),
                ("SCHEMA.json", b"{\"label\":\"Tasks\"}\n".as_slice()),
            ];
        let mut original = BTreeMap::new();
        for (name, bytes) in files {
            fs::write(tasks.join(name), bytes).unwrap();
            original.insert(name, bytes.to_vec());
        }
        let task_store_lock = backup_base.join("tasks.transaction.lock");
        Self {
            _temporary: temporary,
            root,
            backup_base,
            backup,
            task_store_lock,
            original,
            rewrites: crate::users::AssignmentRewrites::new(),
        }
    }

    fn request(&self) -> TaskSchemaMigration<'_> {
        TaskSchemaMigration {
            workspace_id: WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap(),
            workspace_root: &self.root,
            task_store_lock: &self.task_store_lock,
            preexisting_backup_base: &self.backup_base,
            backup_dir: &self.backup,
            legacy_semantic_sync: LegacySemanticSync::Complete,
            assignment_rewrites: &self.rewrites,
        }
    }

    fn assert_live_original(&self) {
        for (name, bytes) in &self.original {
            assert_eq!(
                fs::read(self.root.join("tasks").join(name)).unwrap(),
                *bytes
            );
        }
    }

    fn assert_migrated_and_backed_up(&self) {
        let mut tasks = csv::Reader::from_path(self.root.join("tasks/tasks.csv")).unwrap();
        let task_headers = tasks.headers().unwrap().clone();
        let task_rows = tasks.records().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(task_headers.get(0), Some("task_uuid"));
        assert_eq!(task_rows.len(), 2);
        for (row, display_id) in task_rows.iter().zip(["T42", "T7"]) {
            let expected =
                legacy_task_uuid(self.request().workspace_id, CsvKind::Tasks, display_id)
                    .to_string();
            assert_eq!(row.get(0), Some(expected.as_str()));
            assert_eq!(row.get(1), Some(display_id));
        }
        let mut habits = csv::Reader::from_path(self.root.join("tasks/habits.csv")).unwrap();
        let habit_headers = habits.headers().unwrap().clone();
        let habit_row = habits.records().next().unwrap().unwrap();
        let expected_habit =
            legacy_task_uuid(self.request().workspace_id, CsvKind::Habits, "H3").to_string();
        assert_eq!(habit_headers.get(0), Some("task_uuid"));
        assert_eq!(habit_row.get(0), Some(expected_habit.as_str()));
        assert_eq!(habit_row.get(1), Some("H3"));
        assert_eq!(
            habit_row.get(
                habit_headers
                    .iter()
                    .position(|header| header == "system_key")
                    .unwrap()
            ),
            Some("brain.triage.daily")
        );
        assert_eq!(
            habit_row.get(
                habit_headers
                    .iter()
                    .position(|header| header == "assigned_to")
                    .unwrap()
            ),
            Some("pablo")
        );
        for (name, bytes) in &self.original {
            assert_eq!(
                fs::read(self.backup.join("tasks").join(name)).unwrap(),
                *bytes
            );
        }
    }
}

#[test]
fn crash_after_each_install_boundary_recovers_then_converges_on_retry() {
    for crash_at in [
        MigrationStep::Install(1),
        MigrationStep::Install(2),
        MigrationStep::Commit,
    ] {
        let fixture = Fixture::new();

        let interrupted = catch_unwind(AssertUnwindSafe(|| {
            let _ = migrate_inactive_with_hook(fixture.request(), |step| {
                assert_ne!(step, crash_at, "injected migration crash");
                Ok(())
            });
        }));
        assert!(interrupted.is_err(), "missing crash at {crash_at:?}");

        assert_eq!(
            migrate_inactive(fixture.request()).unwrap(),
            MigrationOutcome::Migrated
        );
        fixture.assert_migrated_and_backed_up();
        assert!(!transaction_journal_path(&fixture.backup).exists());
        assert!(transaction_artifacts(&fixture.root).is_empty());
    }
}

#[test]
fn crash_after_committed_journal_preserves_the_new_generation_on_retry() {
    let fixture = Fixture::new();

    let interrupted = catch_unwind(AssertUnwindSafe(|| {
        let _ = migrate_inactive_with_hook(fixture.request(), |step| {
            assert_ne!(step, MigrationStep::Committed, "injected post-commit crash");
            Ok(())
        });
    }));
    assert!(interrupted.is_err());

    assert_eq!(
        migrate_inactive(fixture.request()).unwrap(),
        MigrationOutcome::AlreadyCurrent
    );
    fixture.assert_migrated_and_backed_up();
    assert!(!transaction_journal_path(&fixture.backup).exists());
    assert!(transaction_artifacts(&fixture.root).is_empty());
}

#[test]
fn install_failure_rolls_back_and_retry_converges() {
    let fixture = Fixture::new();

    let error = migrate_inactive_with_hook(fixture.request(), |step| {
        if step == MigrationStep::Install(1) {
            return Err(io::Error::other("injected install failure"));
        }
        Ok(())
    })
    .unwrap_err();

    assert!(format!("{error:#}").contains("injected install failure"));
    fixture.assert_live_original();
    assert_eq!(
        migrate_inactive(fixture.request()).unwrap(),
        MigrationOutcome::Migrated
    );
    fixture.assert_migrated_and_backed_up();
}

#[test]
fn staging_failure_cleans_temporary_files_before_retry() {
    let fixture = Fixture::new();

    let error = migrate_inactive_with_hook(fixture.request(), |step| {
        if step == MigrationStep::Stage(1) {
            return Err(io::Error::other("injected staging failure"));
        }
        Ok(())
    })
    .unwrap_err();

    assert!(format!("{error:#}").contains("injected staging failure"));
    fixture.assert_live_original();
    assert!(transaction_artifacts(&fixture.root).is_empty());
    assert!(!transaction_journal_path(&fixture.backup).exists());
    assert_eq!(
        migrate_inactive(fixture.request()).unwrap(),
        MigrationOutcome::Migrated
    );
    fixture.assert_migrated_and_backed_up();
}

#[test]
fn backup_parent_open_and_sync_failures_precede_live_replacement_and_are_retryable() {
    for (failed_step, message) in [
        (
            MigrationStep::BackupParentOpen(1),
            "injected backup parent open failure",
        ),
        (
            MigrationStep::BackupParentSync(1),
            "injected backup parent sync failure",
        ),
    ] {
        let fixture = Fixture::new();

        let error = migrate_inactive_with_hook(fixture.request(), |step| {
            if step == failed_step {
                return Err(io::Error::other(message));
            }
            Ok(())
        })
        .unwrap_err();

        assert!(format!("{error:#}").contains(message));
        fixture.assert_live_original();
        assert_eq!(
            fs::read(fixture.backup.join("tasks/tasks.csv")).unwrap(),
            fixture.original["tasks.csv"]
        );
        assert_eq!(
            migrate_inactive(fixture.request()).unwrap(),
            MigrationOutcome::Migrated
        );
        fixture.assert_migrated_and_backed_up();
    }
}

#[test]
fn every_deep_backup_directory_parent_failure_precedes_replacement_and_is_retryable() {
    for failed_index in 0..4 {
        for failed_step in [
            MigrationStep::BackupDirectoryParentOpen(failed_index),
            MigrationStep::BackupDirectoryParentSync(failed_index),
        ] {
            let fixture = Fixture::new();
            let install_started = Cell::new(false);

            let error = migrate_inactive_with_hook(fixture.request(), |step| {
                if matches!(step, MigrationStep::Install(_)) {
                    install_started.set(true);
                }
                if step == failed_step {
                    return Err(io::Error::other("injected backup directory parent failure"));
                }
                Ok(())
            })
            .unwrap_err();

            assert!(format!("{error:#}").contains("injected backup directory parent failure"));
            assert!(!install_started.get());
            fixture.assert_live_original();
            assert_eq!(
                migrate_inactive(fixture.request()).unwrap(),
                MigrationOutcome::Migrated
            );
            fixture.assert_migrated_and_backed_up();
        }
    }
}

#[test]
fn crash_at_each_deep_backup_directory_sync_is_recovered_before_replacement() {
    for crash_index in 0..4 {
        let fixture = Fixture::new();
        let install_started = Cell::new(false);

        let interrupted = catch_unwind(AssertUnwindSafe(|| {
            let _ = migrate_inactive_with_hook(fixture.request(), |step| {
                if matches!(step, MigrationStep::Install(_)) {
                    install_started.set(true);
                }
                assert_ne!(
                    step,
                    MigrationStep::BackupDirectoryParentSync(crash_index),
                    "injected backup directory sync crash"
                );
                Ok(())
            });
        }));

        assert!(interrupted.is_err());
        assert!(!install_started.get());
        fixture.assert_live_original();
        assert_eq!(
            migrate_inactive(fixture.request()).unwrap(),
            MigrationOutcome::Migrated
        );
        fixture.assert_migrated_and_backed_up();
    }
}

#[test]
fn prepared_journal_publish_failure_removes_temporary_file_immediately() {
    let fixture = Fixture::new();

    let error = migrate_inactive_with_hook(fixture.request(), |step| {
        if step == MigrationStep::JournalPublishPrepared {
            return Err(io::Error::other("injected journal publish failure"));
        }
        Ok(())
    })
    .unwrap_err();

    assert!(format!("{error:#}").contains("injected journal publish failure"));
    fixture.assert_live_original();
    assert!(transaction_artifacts(&fixture.root).is_empty());
    assert!(
        !fixture
            .backup
            .join(".brain-task-schema-transaction.json.tmp")
            .exists()
    );
}

fn transaction_artifacts(root: &Path) -> Vec<PathBuf> {
    fs::read_dir(root.join("tasks"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(".brain-task-schema-"))
        })
        .collect()
}
