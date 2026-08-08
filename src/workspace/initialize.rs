//! Safe first-run initialization for an otherwise empty workspace.

use std::path::Path;

use anyhow::{Context, Result};

use super::WorkspaceContext;

const TASKS_HEADER: &str = "task_uuid,task_id,task_name,task_type,status,waiting_since,priority,due_date,hard_deadline,start_date,assigned_to,see_also,notes,project,energy_level,context,estimated_duration,blocked_by,defer_count,created_date,completed_date,last_touched,linear_issue,system_key\n";
const HABITS_HEADER: &str = "task_uuid,task_id,task_name,status,priority,due_date,hard_deadline,assigned_to,see_also,notes,project,energy_level,context,estimated_duration,ideal_time,recur_interval,recur_unit,created_date,completed_date,last_touched,system_key\n";
const PROJECTS_HEADER: &str = "name,namespace,title,status,priority,due,directory\n";
const RESOURCES_HEADER: &str = "zotero_key,title,authors,year,item_type,collection,directory,has_pdf,has_html,has_summary,has_other_notes,annotation_count,tags\n";

const PARA_DIRECTORIES: [&str; 5] = ["projects", "areas", "resources", "archive", "tasks"];
const INFRASTRUCTURE_DIRECTORIES: [&str; 5] = [".config", ".claude", ".codex", ".opencode", ".git"];

/// Initialize the selected workspace when it contains only Brain's own setup
/// directories and no user content. Existing files are never overwritten.
pub(crate) fn initialize_if_empty(workspace: &WorkspaceContext) -> Result<bool> {
    if !is_empty_workspace(workspace.root())? {
        return Ok(false);
    }

    for directory in PARA_DIRECTORIES {
        std::fs::create_dir_all(workspace.root().join(directory))?;
    }
    write_if_missing(&workspace.root().join(".config/config.json"), b"{}\n")?;
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
    crate::tasks::triage_habits::apply_triage_habits_config(workspace, true)?;
    Ok(true)
}

pub(crate) fn is_empty_workspace(root: &Path) -> Result<bool> {
    is_empty_workspace_inner(root)
}

fn is_empty_workspace_inner(root: &Path) -> Result<bool> {
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

fn contains_file(path: std::path::PathBuf) -> Result<bool> {
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

fn write_if_missing(path: &Path, bytes: &[u8]) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::{contains_file, is_empty_workspace_inner};

    #[test]
    fn setup_only_directories_are_empty() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join(".config")).unwrap();
        std::fs::create_dir(root.path().join(".claude")).unwrap();
        std::fs::create_dir(root.path().join("tasks")).unwrap();
        assert!(is_empty_workspace_inner(root.path()).unwrap());
    }

    #[test]
    fn a_user_file_prevents_initialization() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("areas")).unwrap();
        std::fs::write(root.path().join("areas/family.md"), "family").unwrap();
        assert!(!is_empty_workspace_inner(root.path()).unwrap());
    }

    #[test]
    fn nested_empty_para_directories_have_no_files() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("projects/empty")).unwrap();
        assert!(!contains_file(root.path().join("projects")).unwrap());
    }
}
