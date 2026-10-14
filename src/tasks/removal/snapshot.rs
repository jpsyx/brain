//! One owned, read-only observation of the canonical removal inputs.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path};

use chrono::NaiveDate;

use super::fingerprint::CanonicalDocument;
use super::model::{Identity, IntegrationSnapshot, Options, Problem};
use crate::actor::ActorContext;
use crate::tasks::complete::CsvFile;
use crate::tasks::identity::{CsvKind, TaskUuid, legacy_task_uuid};
use crate::tasks::store_lock::TaskStoreOwner;
use crate::workspace::WorkspaceContext;

pub(crate) struct SnapshotContext<'a> {
    pub workspace: &'a WorkspaceContext,
    pub actor: &'a ActorContext,
    pub integration: &'a IntegrationSnapshot,
}

#[derive(Clone, Debug)]
pub(crate) struct CsvSnapshot {
    pub before: Option<Vec<u8>>,
    pub parsed: CsvFile,
    pub identities: Vec<Identity>,
}

#[derive(Clone, Debug)]
pub(crate) struct ProjectSnapshot {
    pub before: Vec<u8>,
    pub metadata: crate::project::model::Metadata,
}

#[derive(Clone, Debug)]
pub(crate) struct Snapshot {
    pub workspace_uuid: String,
    pub actor_id: String,
    pub execution_date: String,
    pub tasks: CsvSnapshot,
    pub habits: CsvSnapshot,
    pub projects: BTreeMap<String, ProjectSnapshot>,
    pub integration: IntegrationSnapshot,
    pub managed_habits_enabled: bool,
    pub warnings: Vec<Problem>,
    pub purge_notes: BTreeMap<String, Option<Vec<u8>>>,
}

impl Snapshot {
    #[cfg(test)]
    pub(crate) fn read(context: &SnapshotContext<'_>, today: NaiveDate) -> Result<Self, Problem> {
        let owner = TaskStoreOwner::acquire(context.workspace).map_err(|error| {
            Problem::new(
                "snapshot_read_failed",
                &error.to_string(),
                serde_json::json!({}),
            )
        })?;
        Self::read_owned(context, today, &owner)
    }

    pub(crate) fn read_owned(
        context: &SnapshotContext<'_>,
        today: NaiveDate,
        owner: &TaskStoreOwner,
    ) -> Result<Self, Problem> {
        let workspace = context.workspace;
        owner
            .verify(workspace)
            .map_err(|error| problem("invalid_task_store_owner", &error, ""))?;
        owner
            .recover(workspace)
            .map_err(|error| problem("snapshot_recovery_failed", &error, ""))?;
        let root = workspace.root();
        validate_path(root, root)?;
        validate_file(root, &root.join(".config/config.json"))?;
        let config = crate::config::Config::try_load(workspace)
            .map_err(|error| problem("invalid_removal_config", &error, ".config/config.json"))?;
        let mut tasks = read_csv(root, "tasks/tasks.csv")?;
        let mut habits = read_csv(root, "tasks/habits.csv")?;
        let mut ids = BTreeMap::new();
        let mut uuids = BTreeMap::new();
        for (kind, csv) in [(CsvKind::Tasks, &mut tasks), (CsvKind::Habits, &mut habits)] {
            for row in &csv.parsed.rows {
                let task_id = crate::tasks::complete::field(row, "task_id")
                    .trim()
                    .to_owned();
                let normalized = crate::tasks::complete::normalize_id(&task_id)
                    .map_err(|error| problem("invalid_task_id", &error, kind.as_str()))?;
                let prefix = match kind {
                    CsvKind::Tasks => 'T',
                    CsvKind::Habits => 'H',
                };
                if !normalized.starts_with(prefix) {
                    return Err(problem(
                        "invalid_task_id",
                        "display ID belongs to another CSV kind",
                        &task_id,
                    ));
                }
                if ids.insert(normalized, task_id.clone()).is_some() {
                    return Err(problem(
                        "duplicate_task_id",
                        "duplicate display ID in task before-images",
                        &task_id,
                    ));
                }
                let stored = crate::tasks::complete::field(row, "task_uuid");
                let task_uuid = if stored.trim().is_empty() {
                    legacy_task_uuid(workspace.id(), kind, &task_id)
                } else {
                    TaskUuid::parse(stored.trim()).map_err(|error| Problem::new(
                        "invalid_task_uuid", &error.to_string(), serde_json::json!({"csv":kind.as_str(), "task_id":task_id, "task_uuid":stored})
                    ))?
                };
                if let Some(previous) = uuids.insert(task_uuid.to_string(), task_id.clone()) {
                    return Err(Problem::new(
                        "duplicate_task_uuid",
                        "duplicate immutable identity in task before-images",
                        serde_json::json!({"task_uuid":task_uuid.to_string(), "task_ids":[previous, task_id]}),
                    ));
                }
                csv.identities.push(Identity {
                    csv: kind,
                    task_id,
                    task_uuid,
                    task_name: crate::tasks::complete::field(row, "task_name"),
                });
            }
        }
        let projects = read_projects(root)?;
        let project_names: std::collections::BTreeSet<_> = projects
            .values()
            .map(|project| project.metadata.name.as_str())
            .collect();
        let referenced: std::collections::BTreeSet<_> = tasks
            .parsed
            .rows
            .iter()
            .chain(&habits.parsed.rows)
            .map(|row| {
                crate::tasks::complete::field(row, "project")
                    .trim()
                    .to_owned()
            })
            .filter(|name| !name.is_empty())
            .collect();
        let warnings = referenced
            .into_iter()
            .filter(|name| !project_names.contains(name.as_str()))
            .map(|name| {
                Problem::new(
                    "missing_project",
                    "referenced project has no scanned metadata",
                    serde_json::json!({"project":name}),
                )
            })
            .collect();
        Ok(Self {
            workspace_uuid: workspace.id().to_string(),
            actor_id: context.actor.user_id().to_string(),
            execution_date: today.format("%Y-%m-%d").to_string(),
            tasks,
            habits,
            projects,
            integration: context.integration.clone(),
            managed_habits_enabled: config.enable_triage_habits,
            warnings,
            purge_notes: BTreeMap::new(),
        })
    }

    pub(crate) fn selected_identities(
        &self,
        needles: &[String],
        options: &Options,
    ) -> Result<Vec<Identity>, Problem> {
        let selected = super::resolve::resolve_for_removal(
            &self.tasks.parsed,
            &self.habits.parsed,
            needles,
            options,
            self.managed_habits_enabled,
        )?;
        Ok(selected
            .into_iter()
            .map(|(kind, index)| match kind {
                crate::tasks::identity::CsvKind::Tasks => self.tasks.identities[index].clone(),
                crate::tasks::identity::CsvKind::Habits => self.habits.identities[index].clone(),
            })
            .collect())
    }

    pub(crate) fn canonical<'a>(
        &'a self,
        options: &'a Options,
        targets: &[Identity],
    ) -> CanonicalDocument<'a> {
        CanonicalDocument::new(self, options, targets)
    }
    pub(crate) fn canonical_for<'a>(
        &'a self,
        options: &'a Options,
        targets: &[Identity],
        intent: Option<&super::maintenance::MaintenanceIntent>,
    ) -> CanonicalDocument<'a> {
        let mut document = self.canonical(options, targets).maintenance(intent);
        if matches!(
            intent,
            Some(super::maintenance::MaintenanceIntent::BacklogPurge { .. })
        ) {
            document.purge_notes = self
                .purge_notes
                .iter()
                .map(|(path, bytes)| (path.as_str(), bytes))
                .collect();
        }
        document
    }
}

fn read_csv(root: &Path, relative: &str) -> Result<CsvSnapshot, Problem> {
    let path = root.join(relative);
    validate_file(root, &path)?;
    let before = match fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(problem("snapshot_read_failed", &error, relative)),
    };
    let parsed = parse_raw_csv(before.as_deref().unwrap_or_default())
        .map_err(|error| problem("invalid_task_csv", &error, relative))?;
    Ok(CsvSnapshot {
        before,
        parsed,
        identities: Vec::new(),
    })
}

/// Removal preserves every original column, including legacy assignment fields.
fn parse_raw_csv(bytes: &[u8]) -> anyhow::Result<CsvFile> {
    let mut reader = csv::ReaderBuilder::new().flexible(true).from_reader(bytes);
    let header: Vec<_> = reader.headers()?.iter().map(str::to_owned).collect();
    anyhow::ensure!(
        header.iter().collect::<BTreeSet<_>>().len() == header.len(),
        "CSV header contains duplicate column names"
    );
    let rows = reader
        .records()
        .enumerate()
        .map(|(index, record)| {
            let record = record?;
            anyhow::ensure!(
                record.len() <= header.len(),
                "CSV record {} has more cells than the header",
                index + 2
            );
            Ok(header
                .iter()
                .enumerate()
                .map(|(index, column)| {
                    (
                        column.clone(),
                        record.get(index).unwrap_or_default().to_owned(),
                    )
                })
                .collect())
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(CsvFile { header, rows })
}

fn read_projects(root: &Path) -> Result<BTreeMap<String, ProjectSnapshot>, Problem> {
    let mut projects = BTreeMap::new();
    for relative in ["projects", "archive/projects"] {
        let directory = root.join(relative);
        validate_path(root, &directory)?;
        if !directory.exists() {
            continue;
        }
        if !directory.is_dir() {
            return Err(problem(
                "invalid_workspace_path",
                "project scan root is not a directory",
                relative,
            ));
        }
        for entry in walkdir::WalkDir::new(&directory)
            .follow_links(false)
            .sort_by_file_name()
        {
            let entry = entry.map_err(|error| problem("snapshot_read_failed", &error, relative))?;
            if entry.file_type().is_symlink() || entry.file_name() != ".METADATA.json" {
                continue;
            }
            let path = entry.path();
            validate_file(root, path)?;
            let relative = path
                .strip_prefix(root)
                .ok()
                .and_then(Path::to_str)
                .ok_or_else(|| {
                    problem(
                        "invalid_workspace_path",
                        "metadata path is not workspace-relative UTF-8",
                        "",
                    )
                })?;
            let before = fs::read(path)
                .map_err(|error| problem("snapshot_read_failed", &error, relative))?;
            let metadata: crate::project::model::Metadata = serde_json::from_slice(&before)
                .map_err(|error| problem("invalid_project_metadata", &error, relative))?;
            projects.insert(relative.to_owned(), ProjectSnapshot { before, metadata });
        }
    }
    Ok(projects)
}

fn validate_path(root: &Path, path: &Path) -> Result<(), Problem> {
    let relative = path
        .strip_prefix(root)
        .map_err(|error| problem("invalid_workspace_path", &error, ""))?;
    let mut current = root.to_path_buf();
    let components = std::iter::once(None).chain(relative.components().map(Some));
    for component in components {
        if let Some(component) = component {
            let Component::Normal(name) = component else {
                return Err(problem(
                    "invalid_workspace_path",
                    "path contains traversal",
                    "",
                ));
            };
            current.push(name);
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(problem(
                    "invalid_workspace_path",
                    "workspace input is a symbolic link",
                    &current.display().to_string(),
                ));
            }
            Ok(metadata) if current == root && !metadata.is_dir() => {
                return Err(problem(
                    "invalid_workspace_path",
                    "workspace root is not a directory",
                    "",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && current != root => {
                return Ok(());
            }
            Err(error) => {
                return Err(problem(
                    "snapshot_read_failed",
                    &error,
                    &current.display().to_string(),
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn validate_file(root: &Path, path: &Path) -> Result<(), Problem> {
    validate_path(root, path)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() => Err(problem(
            "invalid_workspace_path",
            "workspace file input is not a regular file",
            &path.display().to_string(),
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(problem(
            "snapshot_read_failed",
            &error,
            &path.display().to_string(),
        )),
    }
}

fn problem(code: &str, error: impl std::fmt::Display, path: &str) -> Problem {
    Problem::new(code, &error.to_string(), serde_json::json!({"path":path}))
}
