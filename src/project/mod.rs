//! `brain project` — the mechanical half of managing a PARA project.
//!
//! Deciding a project's namespace, its outcome slug, and whether it is really
//! done is judgement, and stays with whoever is doing the thinking. Writing the
//! scaffold, flipping a field, and moving a folder into the archive are not:
//! they are a fixed field set, an exact folder name, and a `mv` that has to
//! keep the path shape. Those are here.
//!
//! Every mutation ends by rebuilding the project lookup, because a
//! `.METADATA.json` the lookup does not know about is the single most common
//! way this tree goes out of sync with itself.

pub(crate) mod model;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

use model::Metadata;

/// Where a project lives, active or archived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Located {
    pub(crate) slug: String,
    pub(crate) directory: PathBuf,
    /// Path relative to the brain root, as `.METADATA.json:directory` records it.
    pub(crate) relative: String,
    pub(crate) archived: bool,
}

pub(crate) fn active_dir(root: &Path, slug: &str) -> PathBuf {
    root.join("projects").join(slug)
}

pub(crate) fn archived_dir(root: &Path, slug: &str) -> PathBuf {
    root.join("archive/projects").join(slug)
}

/// Find `slug` under `projects/`, else under `archive/projects/`.
pub(crate) fn locate(root: &Path, slug: &str) -> Result<Located> {
    let slug = model::validate_slug(slug)?;
    for (directory, relative, archived) in [
        (active_dir(root, &slug), format!("projects/{slug}"), false),
        (
            archived_dir(root, &slug),
            format!("archive/projects/{slug}"),
            true,
        ),
    ] {
        if model::metadata_path(&directory).is_file() {
            return Ok(Located {
                slug,
                directory,
                relative,
                archived,
            });
        }
    }
    bail!("no project '{slug}' under projects/ or archive/projects/")
}

/// What a `new` wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Created {
    pub(crate) slug: String,
    pub(crate) directory: PathBuf,
    pub(crate) metadata: Metadata,
}

/// Scaffold a new project. Refuses to overwrite an existing one.
pub(crate) fn create(
    root: &Path,
    slug: &str,
    title: &str,
    status: &str,
    priority: &str,
    due: &str,
    description: &str,
) -> Result<Created> {
    let slug = model::validate_slug(slug)?;
    let status = model::validate_status(status)?;
    let priority = model::validate_priority(priority)?;
    let due = model::validate_due(due)?;
    if title.trim().is_empty() {
        bail!("a project needs a title");
    }
    let directory = active_dir(root, &slug);
    if directory.exists() {
        bail!("{} already exists", directory.display());
    }
    let metadata = Metadata::new(&slug, title.trim(), &status, &priority, &due);
    model::save(&directory, &metadata)?;
    std::fs::write(
        directory.join("README.md"),
        model::readme(title.trim(), description),
    )?;
    Ok(Created {
        slug,
        directory,
        metadata,
    })
}

/// One field change a `set` made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Change {
    pub(crate) field: &'static str,
    pub(crate) before: String,
    pub(crate) after: String,
}

/// Edit the fields that describe a project's state.
pub(crate) fn set(
    root: &Path,
    slug: &str,
    title: Option<&str>,
    status: Option<&str>,
    priority: Option<&str>,
    due: Option<&str>,
) -> Result<(Located, Vec<Change>)> {
    let located = locate(root, slug)?;
    let mut metadata = model::load(&located.directory)?;
    let requested: Vec<(&'static str, String)> = [
        ("title", title.map(str::trim).map(str::to_owned).map(Ok)),
        ("status", status.map(model::validate_status)),
        ("priority", priority.map(model::validate_priority)),
        ("due", due.map(model::validate_due)),
    ]
    .into_iter()
    .filter_map(|(field, value)| value.map(|value| value.map(|value| (field, value))))
    .collect::<Result<Vec<_>>>()?;
    if requested.is_empty() {
        bail!("no fields given to set (pass at least one)");
    }

    let mut changes = Vec::new();
    for (field, after) in requested {
        let slot = match field {
            "title" => &mut metadata.title,
            "status" => &mut metadata.status,
            "priority" => &mut metadata.priority,
            _ => &mut metadata.due,
        };
        if *slot != after {
            changes.push(Change {
                field,
                before: slot.clone(),
                after: after.clone(),
            });
            *slot = after;
        }
    }
    if !changes.is_empty() {
        // Keep the identity fields honest while we are writing anyway.
        metadata.realign(&located.slug, &located.relative);
        model::save(&located.directory, &metadata)?;
    }
    Ok((located, changes))
}

/// Move a project into the archive, preserving its folder name.
pub(crate) fn archive(root: &Path, slug: &str) -> Result<Located> {
    let located = locate(root, slug)?;
    if located.archived {
        bail!("'{}' is already archived", located.slug);
    }
    let destination = archived_dir(root, &located.slug);
    if destination.exists() {
        bail!("{} already exists", destination.display());
    }
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(&located.directory, &destination)?;

    let relative = format!("archive/projects/{}", located.slug);
    if let Ok(mut metadata) = model::load(&destination) {
        metadata.realign(&located.slug, &relative);
        model::save(&destination, &metadata)?;
    }
    Ok(Located {
        slug: located.slug,
        directory: destination,
        relative,
        archived: true,
    })
}

/// What `brain project show` answers.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct Report {
    pub(crate) slug: String,
    pub(crate) directory: String,
    pub(crate) archived: bool,
    pub(crate) title: String,
    pub(crate) status: String,
    pub(crate) priority: String,
    pub(crate) due: String,
    pub(crate) open_tasks: Vec<String>,
    /// Open tasks the chronic-ignore sweep flags.
    pub(crate) ignored_tasks: Vec<String>,
    /// Every open task has been ignored — the project probably died quietly.
    pub(crate) died_quietly: bool,
}

/// Describe a project, including whether it looks abandoned.
///
/// "Every open task under it has been ignored for weeks" is the signal that a
/// project stopped rather than finished. It does not block anything; it exists
/// so archiving is a decision rather than a way of papering over rot.
pub(crate) fn show(root: &Path, slug: &str, today: chrono::NaiveDate) -> Result<Report> {
    let located = locate(root, slug)?;
    let metadata = model::load(&located.directory)?;
    let tasks = crate::tasks::complete::read_csv(&root.join("tasks/tasks.csv"))?;

    let mut open_tasks = Vec::new();
    let mut ignored_tasks = Vec::new();
    for row in &tasks.rows {
        if crate::tasks::complete::field(row, "project").trim() != located.slug {
            continue;
        }
        let status = crate::tasks::complete::field(row, "status");
        if matches!(status.trim(), "done" | "backlog") {
            continue;
        }
        let id = crate::tasks::complete::field(row, "task_id");
        if crate::tasks::scan::chronic::classify(row, today).is_some() {
            ignored_tasks.push(id.clone());
        }
        open_tasks.push(id);
    }

    Ok(Report {
        slug: located.slug,
        directory: located.relative,
        archived: located.archived,
        title: metadata.title,
        status: metadata.status,
        priority: metadata.priority,
        due: metadata.due,
        died_quietly: !open_tasks.is_empty() && ignored_tasks.len() == open_tasks.len(),
        open_tasks,
        ignored_tasks,
    })
}
