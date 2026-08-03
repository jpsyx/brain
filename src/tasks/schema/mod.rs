//! Inactive portable task-schema migration primitives.
//!
//! This module performs no discovery and is not called by bootstrap, task
//! commands, readiness, or sync. The coordinated rollout supplies an explicit
//! legacy-sync precondition, workspace root, and machine-local backup path.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};

use super::identity::CsvKind;
use crate::workspace::WorkspaceId;

mod path;
mod transaction;
mod transform;

use path::validate_backup_destination;
#[cfg(test)]
use transaction::journal_path as transaction_journal_path;
use transaction::{FileChange, MigrationStep, recover_pending, replace_group};
use transform::{is_current, migrate_csv, migrate_schema_metadata};

pub const TASK_SCHEMA_VERSION: u64 = 2;

const PORTABLE_FILES: [&str; 5] = [
    "tasks.csv",
    "habits.csv",
    ".tasks_next_id",
    ".habits_next_id",
    "SCHEMA.json",
];

/// Rollout-owned status of the required last legacy semantic sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacySemanticSync {
    /// Sync is configured and the rollout has not completed its final legacy pass.
    Required,
    /// The rollout completed and journaled the final legacy semantic sync.
    Complete,
    /// This workspace has no configured sync transport.
    NotConfigured,
}

/// All explicit capabilities needed to invoke the otherwise inactive helper.
#[derive(Debug, Clone, Copy)]
pub struct TaskSchemaMigration<'a> {
    pub workspace_id: WorkspaceId,
    pub workspace_root: &'a Path,
    pub backup_dir: &'a Path,
    pub legacy_semantic_sync: LegacySemanticSync,
}

/// Whether an inactive migration changed its fixture/workspace inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationOutcome {
    Migrated,
    AlreadyCurrent,
}

/// Apply the schema conversion only after a rollout coordinator supplies the
/// legacy-sync decision and a machine-local backup destination.
pub fn migrate_inactive(request: TaskSchemaMigration<'_>) -> Result<MigrationOutcome> {
    migrate_inactive_with_hook(request, |_| Ok(()))
}

fn migrate_inactive_with_hook(
    request: TaskSchemaMigration<'_>,
    mut hook: impl FnMut(MigrationStep) -> std::io::Result<()>,
) -> Result<MigrationOutcome> {
    let (workspace_root, backup_dir) =
        validate_backup_destination(request.workspace_root, request.backup_dir)?;
    recover_pending(
        &workspace_root,
        &backup_dir,
        request.workspace_id,
        &mut hook,
    )?;
    let tasks_dir = workspace_root.join("tasks");
    let tasks_path = tasks_dir.join("tasks.csv");
    let habits_path = tasks_dir.join("habits.csv");
    let schema_path = tasks_dir.join("SCHEMA.json");
    let tasks_bytes = read_required(&tasks_path)?;
    let habits_bytes = read_required(&habits_path)?;
    let schema_bytes = read_required(&schema_path)?;

    if is_current(&tasks_bytes, &habits_bytes, &schema_bytes)? {
        return Ok(MigrationOutcome::AlreadyCurrent);
    }
    if request.legacy_semantic_sync == LegacySemanticSync::Required {
        bail!(
            "legacy semantic sync must be completed by the coordinated rollout before task UUID migration"
        );
    }

    back_up_portable_files(&tasks_dir, &backup_dir, &mut hook)?;
    let migrated_tasks = migrate_csv(&tasks_bytes, request.workspace_id, CsvKind::Tasks)?;
    let migrated_habits = migrate_csv(&habits_bytes, request.workspace_id, CsvKind::Habits)?;
    let migrated_schema = migrate_schema_metadata(&schema_bytes)?;
    replace_group(
        &workspace_root,
        &backup_dir,
        request.workspace_id,
        &[
            FileChange {
                name: "tasks.csv",
                before: tasks_bytes,
                after: migrated_tasks,
            },
            FileChange {
                name: "habits.csv",
                before: habits_bytes,
                after: migrated_habits,
            },
            FileChange {
                name: "SCHEMA.json",
                before: schema_bytes,
                after: migrated_schema,
            },
        ],
        &mut hook,
    )?;
    Ok(MigrationOutcome::Migrated)
}

fn read_required(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).with_context(|| format!("reading required task schema input {}", path.display()))
}

fn back_up_portable_files(
    tasks_dir: &Path,
    backup_dir: &Path,
    hook: &mut impl FnMut(MigrationStep) -> std::io::Result<()>,
) -> Result<()> {
    let destination_dir = backup_dir.join("tasks");
    fs::create_dir_all(&destination_dir).with_context(|| {
        format!(
            "creating task migration backup {}",
            destination_dir.display()
        )
    })?;
    sync_parent(backup_dir)?;
    sync_parent(&destination_dir)?;
    for (index, name) in PORTABLE_FILES.into_iter().enumerate() {
        let source = tasks_dir.join(name);
        if !source.exists() {
            continue;
        }
        let bytes = fs::read(&source)
            .with_context(|| format!("reading task migration backup input {}", source.display()))?;
        let destination = destination_dir.join(name);
        if destination.exists() {
            let existing = fs::read(&destination).with_context(|| {
                format!(
                    "reading existing task migration backup {}",
                    destination.display()
                )
            })?;
            if existing != bytes {
                bail!(
                    "task migration backup already exists with different bytes: {}",
                    destination.display()
                );
            }
        } else {
            write_new(&destination, &bytes)?;
        }
        sync_backup_parent(&destination, index, hook)?;
    }
    Ok(())
}

fn sync_backup_parent(
    path: &Path,
    index: usize,
    hook: &mut impl FnMut(MigrationStep) -> std::io::Result<()>,
) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("task migration backup has no parent: {}", path.display()))?;
    hook(MigrationStep::BackupParentOpen(index))
        .with_context(|| format!("opening task migration backup parent {}", parent.display()))?;
    let directory = fs::File::open(parent)
        .with_context(|| format!("opening task migration backup parent {}", parent.display()))?;
    hook(MigrationStep::BackupParentSync(index))
        .with_context(|| format!("syncing task migration backup parent {}", parent.display()))?;
    directory
        .sync_all()
        .with_context(|| format!("syncing task migration backup parent {}", parent.display()))
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("creating {}", path.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("writing {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("syncing {}", path.display()))?;
    Ok(())
}

fn sync_parent(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("task migration path has no parent: {}", path.display()))?;
    let directory = fs::File::open(parent)
        .with_context(|| format!("opening task migration parent {}", parent.display()))?;
    directory
        .sync_all()
        .with_context(|| format!("syncing task migration parent {}", parent.display()))
}

#[cfg(test)]
mod transaction_tests {
    use std::collections::BTreeMap;
    use std::io;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::path::PathBuf;

    use super::*;
    use crate::tasks::identity::legacy_task_uuid;

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
            let tasks = root.join("tasks");
            let backup = temporary.path().join("machine-local/task-schema");
            fs::create_dir_all(&tasks).unwrap();
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
            Self {
                _temporary: temporary,
                root,
                backup,
                original,
            }
        }

        fn request(&self) -> TaskSchemaMigration<'_> {
            TaskSchemaMigration {
                workspace_id: WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap(),
                workspace_root: &self.root,
                backup_dir: &self.backup,
                legacy_semantic_sync: LegacySemanticSync::Complete,
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
}
