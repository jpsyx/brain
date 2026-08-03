use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::sync::csv_merge::{Table, project_task_lists, rewrite_project_metadata};

pub(super) struct MetadataUpdate {
    path: PathBuf,
    relative: String,
    before: Vec<u8>,
    after: Vec<u8>,
    locally_changed: bool,
}

#[derive(Debug)]
pub(super) enum MetadataPublishError {
    Local(String),
    Remote(String),
}

pub(super) fn prepare_project_metadata(
    root: &Path,
    tables: &[Table],
) -> Result<Vec<MetadataUpdate>> {
    let project_ids = project_task_lists(tables.iter());
    let mut staged = Vec::new();
    for base in [root.join("projects"), root.join("archive/projects")] {
        let Ok(entries) = std::fs::read_dir(base) else {
            continue;
        };
        let mut metadata_paths = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path().join(".METADATA.json"))
            .filter(|path| path.is_file())
            .collect::<Vec<_>>();
        metadata_paths.sort();
        for path in metadata_paths {
            let before = std::fs::read(&path)
                .with_context(|| format!("reading project metadata {}", path.display()))?;
            let value = serde_json::from_slice::<serde_json::Value>(&before)
                .with_context(|| format!("parsing project metadata {}", path.display()))?;
            let project = value
                .get("name")
                .and_then(serde_json::Value::as_str)
                .or_else(|| path.parent()?.file_name()?.to_str())
                .unwrap_or_default();
            let ids = project_ids.get(project).map_or(&[][..], Vec::as_slice);
            let after = rewrite_project_metadata(&before, ids)
                .with_context(|| format!("rewriting project metadata {}", path.display()))?;
            let relative = path
                .strip_prefix(root)
                .with_context(|| {
                    format!(
                        "project metadata {} is outside the workspace",
                        path.display()
                    )
                })?
                .to_string_lossy()
                .replace('\\', "/");
            staged.push(MetadataUpdate {
                path,
                relative,
                locally_changed: before != after,
                before,
                after,
            });
        }
    }
    Ok(staged)
}

pub(super) fn publish_project_metadata(
    staged: &[MetadataUpdate],
    update_local: bool,
    mut push: impl FnMut(&str, &str) -> bool,
) -> std::result::Result<usize, MetadataPublishError> {
    let mut written = Vec::new();
    if update_local {
        for update in staged {
            if !update.locally_changed {
                continue;
            }
            if let Err(error) = std::fs::write(&update.path, &update.after) {
                for (written_path, original) in written.into_iter().rev() {
                    let _ = std::fs::write(written_path, original);
                }
                return Err(MetadataPublishError::Local(format!(
                    "writing project metadata {}: {error}",
                    update.path.display()
                )));
            }
            written.push((&update.path, &update.before));
        }
    }
    for update in staged {
        let text = String::from_utf8_lossy(&update.after);
        if !push(&update.relative, &text) {
            return Err(MetadataPublishError::Remote(update.relative.clone()));
        }
    }
    Ok(staged
        .iter()
        .filter(|update| update.locally_changed)
        .count())
}

#[cfg(test)]
pub(super) fn reconcile_project_metadata(
    root: &Path,
    tables: &[Table],
    update_local: bool,
    push: impl FnMut(&str, &str) -> bool,
) -> Result<usize> {
    let staged = prepare_project_metadata(root, tables)?;
    publish_project_metadata(&staged, update_local, push)
        .map_err(|error| anyhow::anyhow!("{error:?}"))
}
