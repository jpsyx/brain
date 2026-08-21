use std::path::Path;

use anyhow::{Context, Result};

use super::{
    HABITS_HEADER, INFRASTRUCTURE_DIRECTORIES, PARA_DIRECTORIES, PROJECTS_HEADER, RESOURCES_HEADER,
    TASKS_HEADER, WorkspaceContext,
};

/// Initialize the selected workspace when it contains only Brain's own setup
/// directories and no user content. Existing files are never overwritten.
pub(crate) fn initialize_if_empty(workspace: &WorkspaceContext) -> Result<bool> {
    if !is_empty_workspace(workspace.root())? {
        return Ok(false);
    }
    seed_empty_workspace(workspace)?;
    Ok(true)
}

/// The task store the sync's CSV lane requires, written before the first sync.
///
/// Only the files that lane reads: both CSVs, both id counters, and (through
/// [`resolve_task_schema_document`]) the schema document. The rest of the
/// skeleton can wait until after the sync.
pub(super) fn seed_task_store(root: &Path) -> Result<()> {
    std::fs::create_dir_all(root.join("tasks"))?;
    write_if_missing(&root.join("tasks/tasks.csv"), TASKS_HEADER.as_bytes())?;
    write_if_missing(&root.join("tasks/habits.csv"), HABITS_HEADER.as_bytes())?;
    write_if_missing(&root.join("tasks/.tasks_next_id"), b"1\n")?;
    write_if_missing(&root.join("tasks/.habits_next_id"), b"1\n")?;
    Ok(())
}

/// Give this machine the workspace's task schema document before anything reads it.
///
/// The remote's document is the authority when there is one: a workspace may
/// carry a customized schema, and the document is excluded from bisync, so
/// seeding Brain's canonical copy over a customized remote would fork the two
/// with nothing able to reconcile them. Brain's canonical document is the
/// fallback for a remote that has none.
pub(super) fn resolve_task_schema_document(
    workspace: &WorkspaceContext,
    config: &crate::sync::config::SyncConfig,
) -> Result<()> {
    let root = workspace.root();
    if crate::tasks::schema::document_present(root) {
        return Ok(());
    }
    if config.is_configured()
        && let Some(document) = adopted_remote_schema_document(workspace, config)
    {
        std::fs::create_dir_all(root.join("tasks"))?;
        write_if_missing(&root.join("tasks/SCHEMA.json"), document.as_bytes())?;
        eprintln!(
            "{}",
            crate::theme::Theme::active().success(&format!(
                "Adopted {}'s task schema from the remote",
                workspace.name().as_str()
            ))
        );
        return Ok(());
    }
    crate::tasks::schema::ensure_schema_document(root)?;
    Ok(())
}

/// The remote's task schema document, or `None` for any reason at all.
///
/// An unreachable or uninitialized remote is not an error here: the sync that
/// follows reports it with its own message, and the canonical document is a
/// correct local answer meanwhile.
fn adopted_remote_schema_document(
    workspace: &WorkspaceContext,
    config: &crate::sync::config::SyncConfig,
) -> Option<String> {
    let remote = crate::sync::remote::build_remote(config);
    let verified =
        crate::sync::identity::require_remote_identity(workspace.root(), workspace.id(), &remote)
            .ok()?;
    crate::sync::csv_sync::fetch_remote_task_schema(workspace.paths(), verified.remote())
        .ok()
        .flatten()
        .filter(|document| !document.trim().is_empty())
}

pub(super) fn seed_empty_workspace(workspace: &WorkspaceContext) -> Result<()> {
    for directory in PARA_DIRECTORIES {
        std::fs::create_dir_all(workspace.root().join(directory))?;
    }
    write_if_missing(&workspace.root().join(".config/config.json"), b"{}\n")?;
    // What this directory is, for an agent and for a person. Seeded only for a
    // workspace Brain is creating: a root that already has content has its own
    // conventions, and dropping instructions into it would presume to describe
    // material Brain has never seen.
    crate::workspace::templates::seed_documents(workspace.root())?;
    write_if_missing(
        &workspace.root().join("tasks/tasks.csv"),
        TASKS_HEADER.as_bytes(),
    )?;
    write_if_missing(
        &workspace.root().join("tasks/habits.csv"),
        HABITS_HEADER.as_bytes(),
    )?;
    write_if_missing(
        &workspace.root().join("projects/projects-lookup.csv"),
        PROJECTS_HEADER.as_bytes(),
    )?;
    write_if_missing(
        &workspace.root().join("resources/zotero-lookup.csv"),
        RESOURCES_HEADER.as_bytes(),
    )?;
    write_if_missing(&workspace.root().join("tasks/.tasks_next_id"), b"1\n")?;
    write_if_missing(&workspace.root().join("tasks/.habits_next_id"), b"1\n")?;
    crate::tasks::schema::ensure_schema_document(workspace.root())?;
    // Honor an existing answer instead of forcing the default on: a workspace
    // can carry portable config (a synced or freshly configured one does)
    // before it has any content, and seeding must not overwrite a deliberate
    // `enable_triage_habits: false`. A workspace with no config at all still
    // gets the default, which is on.
    let enable_triage_habits = crate::config::Config::load(workspace).enable_triage_habits;
    crate::tasks::triage_habits::apply_triage_habits_config(workspace, enable_triage_habits)?;
    Ok(())
}

pub(crate) fn is_empty_workspace(root: &Path) -> Result<bool> {
    is_empty_workspace_inner(root)
}

pub(super) fn is_empty_workspace_inner(root: &Path) -> Result<bool> {
    let entries = std::fs::read_dir(root)
        .with_context(|| format!("inspect workspace root {}", root.display()))?;
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if INFRASTRUCTURE_DIRECTORIES.contains(&name.as_ref()) || name == ".DS_Store" {
            continue;
        }
        if PARA_DIRECTORIES.contains(&name.as_ref()) {
            if contains_file(entry.path())? {
                return Ok(false);
            }
            continue;
        }
        return Ok(false);
    }
    Ok(true)
}

pub(super) fn contains_file(path: std::path::PathBuf) -> Result<bool> {
    if path.is_file() {
        return Ok(true);
    }
    if !path.is_dir() {
        return Ok(false);
    }
    for entry in std::fs::read_dir(path)? {
        if contains_file(entry?.path())? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn write_if_missing(path: &Path, bytes: &[u8]) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("{} has no parent directory", path.display()))?;
    std::fs::create_dir_all(parent)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    let mut file = options
        .open(path)
        .with_context(|| format!("create {}", path.display()))?;
    std::io::Write::write_all(&mut file, bytes)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}
