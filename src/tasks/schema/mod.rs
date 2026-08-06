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
use transform::{
    is_current, migrate_csv, migrate_schema_metadata, repair_duplicate_uuids, schema_version,
};

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
    /// Legacy `assigned_to` values the rollout mapped onto portable members.
    pub assignment_rewrites: &'a crate::users::AssignmentRewrites,
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
    let migrated_tasks = migrate_csv(
        &tasks_bytes,
        request.workspace_id,
        CsvKind::Tasks,
        request.assignment_rewrites,
    )?;
    let migrated_habits = migrate_csv(
        &habits_bytes,
        request.workspace_id,
        CsvKind::Habits,
        request.assignment_rewrites,
    )?;
    let (migrated_tasks, migrated_habits) =
        repair_duplicate_uuids(&migrated_tasks, &migrated_habits, request.workspace_id)?;
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

/// Repair duplicate UUIDs in a current workspace left by older writers.
pub(crate) fn repair_current_duplicate_uuids(
    workspace_root: &Path,
    workspace_id: WorkspaceId,
) -> Result<bool> {
    let tasks_path = workspace_root.join("tasks/tasks.csv");
    let habits_path = workspace_root.join("tasks/habits.csv");
    let tasks = read_required(&tasks_path)?;
    let habits = read_required(&habits_path)?;
    let (repaired_tasks, repaired_habits) = repair_duplicate_uuids(&tasks, &habits, workspace_id)?;
    if repaired_tasks == tasks && repaired_habits == habits {
        return Ok(false);
    }
    let tasks_temporary =
        tasks_path.with_file_name(format!(".tasks.csv.repair-{}.tmp", WorkspaceId::new()));
    let habits_temporary =
        habits_path.with_file_name(format!(".habits.csv.repair-{}.tmp", WorkspaceId::new()));
    let result = (|| {
        write_new(&tasks_temporary, &repaired_tasks)?;
        write_new(&habits_temporary, &repaired_habits)?;
        fs::rename(&tasks_temporary, &tasks_path)
            .with_context(|| format!("publishing repaired {}", tasks_path.display()))?;
        sync_parent(&tasks_path)?;
        fs::rename(&habits_temporary, &habits_path)
            .with_context(|| format!("publishing repaired {}", habits_path.display()))?;
        sync_parent(&habits_path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tasks_temporary);
        let _ = fs::remove_file(&habits_temporary);
    }
    result.map(|()| true)
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
mod transaction_tests;
