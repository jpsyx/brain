//! Inactive portable task-schema migration primitives.
//!
//! This module performs no discovery and is not called by bootstrap, task
//! commands, readiness, or sync. The coordinated rollout supplies an explicit
//! legacy-sync precondition, workspace root, pre-existing durable backup base,
//! and a machine-local backup path beneath that base.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};

use super::identity::CsvKind;
use crate::workspace::WorkspaceId;

mod columns;
mod path;
mod transaction;
mod transform;

pub(crate) use columns::{canonical_current_header, is_known_current_column};
use path::validate_backup_destination;
#[cfg(test)]
use transaction::journal_path as transaction_journal_path;
use transaction::{FileChange, MigrationStep, recover_pending, replace_group};
use transform::{is_current, migrate_csv, migrate_schema_metadata, schema_version};

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
    /// UUID-scoped interprocess task-store ownership path.
    pub task_store_lock: &'a Path,
    /// Existing machine-local directory whose entry is already durable.
    ///
    /// The migration creates `backup_dir` beneath this base one component at a
    /// time and syncs every parent. Callers must create and durably publish the
    /// base before invoking this helper.
    pub preexisting_backup_base: &'a Path,
    /// Machine-local destination at or below `preexisting_backup_base`.
    pub backup_dir: &'a Path,
    pub legacy_semantic_sync: LegacySemanticSync,
}

/// Whether an inactive migration changed its fixture/workspace inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationOutcome {
    Migrated,
    AlreadyCurrent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Inspection {
    pub(crate) version: Option<u64>,
    pub(crate) current: bool,
}

pub(crate) fn inspect_inactive(workspace_root: &Path) -> Result<Inspection> {
    let tasks_dir = workspace_root.join("tasks");
    let tasks = read_required(&tasks_dir.join("tasks.csv"))?;
    let habits = read_required(&tasks_dir.join("habits.csv"))?;
    let schema = read_required(&tasks_dir.join("SCHEMA.json"))?;
    Ok(Inspection {
        version: schema_version(&schema)?,
        current: is_current(&tasks, &habits, &schema)?,
    })
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
    let _task_owner =
        crate::tasks::store_lock::TaskStoreOwner::acquire_path(request.task_store_lock)?;
    let (workspace_root, backup_base, backup_dir) = validate_backup_destination(
        request.workspace_root,
        request.preexisting_backup_base,
        request.backup_dir,
    )?;
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

    if let Some(found) = schema_version(&schema_bytes)?
        && found > TASK_SCHEMA_VERSION
    {
        bail!(
            "task schema {found} is newer than supported schema {TASK_SCHEMA_VERSION}; refusing migration"
        );
    }
    if is_current(&tasks_bytes, &habits_bytes, &schema_bytes)? {
        return Ok(MigrationOutcome::AlreadyCurrent);
    }
    if request.legacy_semantic_sync == LegacySemanticSync::Required {
        bail!(
            "legacy semantic sync must be completed by the coordinated rollout before task UUID migration"
        );
    }

    back_up_portable_files(&tasks_dir, &backup_base, &backup_dir, &mut hook)?;
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
    backup_base: &Path,
    backup_dir: &Path,
    hook: &mut impl FnMut(MigrationStep) -> std::io::Result<()>,
) -> Result<()> {
    let destination_dir = backup_dir.join("tasks");
    ensure_durable_directory_chain(backup_base, &destination_dir, hook)?;
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

fn ensure_durable_directory_chain(
    base: &Path,
    destination: &Path,
    hook: &mut impl FnMut(MigrationStep) -> std::io::Result<()>,
) -> Result<()> {
    let relative = destination.strip_prefix(base).with_context(|| {
        format!(
            "task migration backup {} is outside durable base {}",
            destination.display(),
            base.display()
        )
    })?;
    let mut parent = base.to_path_buf();
    for (index, component) in relative.components().enumerate() {
        let child = parent.join(component);
        match fs::create_dir(&child) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && child.is_dir() => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "creating task migration backup directory {}",
                        child.display()
                    )
                });
            }
        }
        sync_backup_directory_parent(&child, index, hook)?;
        parent = child;
    }
    Ok(())
}

fn sync_backup_directory_parent(
    path: &Path,
    index: usize,
    hook: &mut impl FnMut(MigrationStep) -> std::io::Result<()>,
) -> Result<()> {
    let parent = path.parent().ok_or_else(|| {
        anyhow!(
            "task migration backup directory has no parent: {}",
            path.display()
        )
    })?;
    hook(MigrationStep::BackupDirectoryParentOpen(index)).with_context(|| {
        format!(
            "opening task migration backup directory parent {}",
            parent.display()
        )
    })?;
    let directory = fs::File::open(parent).with_context(|| {
        format!(
            "opening task migration backup directory parent {}",
            parent.display()
        )
    })?;
    hook(MigrationStep::BackupDirectoryParentSync(index)).with_context(|| {
        format!(
            "syncing task migration backup directory parent {}",
            parent.display()
        )
    })?;
    directory.sync_all().with_context(|| {
        format!(
            "syncing task migration backup directory parent {}",
            parent.display()
        )
    })
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
}
