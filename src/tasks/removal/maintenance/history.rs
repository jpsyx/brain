//! Scoped purge before-images and deterministic project composition.
use super::MaintenanceIntent;
use crate::{
    tasks::{
        backlog::purge,
        complete::{field, parse_date},
        removal::{
            execute::problem,
            model::{FileEdit, Identity, Problem},
            snapshot::{Snapshot, validate_file},
        },
    },
    workspace::WorkspaceContext,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub(in crate::tasks::removal) fn project_for(
    snapshot: &Snapshot,
    slug: &str,
) -> Result<Option<String>, Problem> {
    if slug.is_empty() {
        return Ok(None);
    }
    let active = format!("projects/{slug}/.METADATA.json");
    if snapshot.projects.contains_key(&active) {
        return Ok(Some(active));
    }
    let archived: Vec<_> = snapshot
        .projects
        .keys()
        .filter(|path| {
            path.starts_with("archive/projects/")
                && Path::new(path)
                    .parent()
                    .and_then(Path::file_name)
                    .is_some_and(|name| name == slug)
        })
        .cloned()
        .collect();
    match archived.as_slice() {
        [] => Ok(None),
        [path] => Ok(Some(path.clone())),
        _ => Err(problem(
            "ambiguous_project",
            "Several archived projects match the purge task",
        )),
    }
}
fn groups(
    snapshot: &Snapshot,
    targets: &[Identity],
) -> Result<BTreeMap<String, Vec<purge::PurgedTask>>, Problem> {
    let mut result: BTreeMap<String, Vec<purge::PurgedTask>> = BTreeMap::new();
    for target in targets {
        let row = snapshot
            .tasks
            .parsed
            .rows
            .iter()
            .find(|row| field(row, "task_id").trim() == target.task_id)
            .ok_or_else(|| problem("invalid_maintenance", "Selected purge task is absent"))?;
        let slug = field(row, "project").trim().to_owned();
        if let Some(path) = project_for(snapshot, &slug)? {
            result.entry(path).or_default().push(purge::PurgedTask {
                task_id: target.task_id.clone(),
                task_name: field(row, "task_name"),
                backlogged_date: field(row, "backlogged_date"),
                project: slug,
            });
        }
    }
    for tasks in result.values_mut() {
        tasks.sort_by(|a, b| a.task_id.cmp(&b.task_id));
    }
    Ok(result)
}
pub(in crate::tasks::removal) fn enrich(
    snapshot: &mut Snapshot,
    workspace: &WorkspaceContext,
    targets: &[Identity],
    intent: Option<&MaintenanceIntent>,
) -> Result<(), Problem> {
    if !matches!(intent, Some(MaintenanceIntent::BacklogPurge { .. })) {
        return Ok(());
    }
    for metadata in groups(snapshot, targets)?.keys() {
        let path = Path::new(metadata).with_file_name("notes.md");
        let full = workspace.root().join(&path);
        validate_file(workspace.root(), &full)?;
        let bytes = match std::fs::read(full) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => {
                return Err(problem(
                    "snapshot_read_failed",
                    "Cannot read selected project notes",
                ));
            }
        };
        if bytes
            .as_ref()
            .is_some_and(|bytes| std::str::from_utf8(bytes).is_err())
        {
            return Err(problem(
                "invalid_project_notes",
                "Selected project notes are not UTF-8",
            ));
        }
        snapshot
            .purge_notes
            .insert(path.to_string_lossy().into_owned(), bytes);
    }
    Ok(())
}
pub(in crate::tasks::removal) fn project_edits(
    snapshot: &Snapshot,
    targets: &[Identity],
    removed: &BTreeSet<String>,
    intent: Option<&MaintenanceIntent>,
) -> Result<Vec<FileEdit>, Problem> {
    let mut history = BTreeMap::new();
    let mut edits = Vec::new();
    if matches!(intent, Some(MaintenanceIntent::BacklogPurge { .. })) {
        let today = parse_date(&snapshot.execution_date)
            .ok_or_else(|| problem("invalid_maintenance", "Invalid deletion date"))?;
        for (metadata, tasks) in groups(snapshot, targets)? {
            history.insert(
                metadata.clone(),
                tasks
                    .iter()
                    .map(|task| purge::history_entry(task, today))
                    .collect(),
            );
            let notes = Path::new(&metadata).with_file_name("notes.md");
            let before = snapshot
                .purge_notes
                .get(notes.to_str().unwrap_or_default())
                .ok_or_else(|| {
                    problem(
                        "invalid_maintenance",
                        "Selected notes before-image is missing",
                    )
                })?;
            let text =
                std::str::from_utf8(before.as_deref().unwrap_or_default()).map_err(|_| {
                    problem(
                        "invalid_project_notes",
                        "Selected project notes are not UTF-8",
                    )
                })?;
            edits.push(FileEdit {
                relative: notes,
                before: before.clone(),
                after: purge::append_breadcrumbs(text, &tasks, today).into_bytes(),
            });
        }
    }
    for (relative, after) in crate::tasks::removal::projects::compose(
        &snapshot.projects,
        removed,
        &super::replacements(intent),
        &history,
    )? {
        edits.push(FileEdit {
            before: Some(snapshot.projects[&relative].before.clone()),
            relative: relative.into(),
            after,
        });
    }
    Ok(edits)
}
