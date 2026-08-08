//! The raw JSON config store at `<brain-root>/.config/config.json`: locating it
//! and reading/writing the whole object. A broken or missing file never blocks
//! startup — it reads as an empty map.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::{Map, Value};

/// The brain config directory: `<brain-root>/.config`.
///
/// This is the home for everything brain persists — the JSON config store,
/// `personalization.json`, and the skill `extensions/` and `plugins/` sources.
/// It lives **inside the brain root**, so it travels with the brain (whatever
/// syncs the brain dir syncs the config too) and no external dotfiles manager
/// has to be involved. The one exception is the brain-root pointer itself,
/// which can't live inside the root (see [`crate::paths`]).
#[must_use]
pub fn config_dir(workspace: &crate::workspace::WorkspaceContext) -> PathBuf {
    config_dir_at(workspace.root())
}

/// The brain config directory for an explicit workspace `root`, for callers that
/// hold a registry record instead of the selected [`WorkspaceContext`].
///
/// [`WorkspaceContext`]: crate::workspace::WorkspaceContext
#[must_use]
pub(crate) fn config_dir_at(root: &Path) -> PathBuf {
    root.join(".config")
}

/// Absolute path to the JSON config store.
#[must_use]
pub fn store_path(workspace: &crate::workspace::WorkspaceContext) -> PathBuf {
    store_path_at(workspace.root())
}

/// Absolute path to the JSON config store under an explicit workspace `root`.
#[must_use]
pub(crate) fn store_path_at(root: &Path) -> PathBuf {
    config_dir_at(root).join("config.json")
}

/// Read a JSON object at an explicit `path`. A missing, unreadable, or
/// non-object file yields an empty map — a broken config never blocks
/// startup. Split out of [`load_map`] so the env migration
/// (`env::migrate`, a different top-level module) can read `config.json` at
/// an explicit brain-root dir hermetically, without going through the real
/// `store_path()`.
#[must_use]
pub(crate) fn load_map_at(path: &Path) -> Map<String, Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default()
}

/// Read the store as a JSON object. A missing, unreadable, or non-object file
/// yields an empty map — a broken config never blocks startup.
#[must_use]
pub(crate) fn load_map(workspace: &crate::workspace::WorkspaceContext) -> Map<String, Value> {
    load_map_at_root(workspace.root())
}

/// Read the config store belonging to an explicit workspace `root`. Lets the env
/// breakdown resolve legacy config fallbacks for a workspace that is not the
/// selected one.
#[must_use]
pub(crate) fn load_map_at_root(root: &Path) -> Map<String, Value> {
    load_map_at(&store_path_at(root))
}

/// Write `map` as the JSON object at an explicit `path`, creating parent dirs
/// as needed. Split out of [`save_map`] for the same reason as
/// [`load_map_at`].
pub(crate) fn save_map_at(path: &Path, map: &Map<String, Value>) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let body = serde_json::to_string_pretty(&Value::Object(map.clone()))?;
    std::fs::write(path, format!("{body}\n"))?;
    Ok(())
}

pub(super) fn save_map(
    workspace: &crate::workspace::WorkspaceContext,
    map: &Map<String, Value>,
    owner: &crate::tasks::store_lock::TaskStoreOwner,
) -> Result<()> {
    owner.verify(workspace)?;
    save_map_at(&store_path(workspace), map)
}

pub(super) fn home_dir() -> PathBuf {
    std::env::var_os("HOME").map_or_else(PathBuf::new, PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_path_is_config_json_inside_the_brain_root() {
        let workspace = crate::workspace::WorkspaceContext::new(
            Path::new("/home/tester"),
            crate::workspace::WorkspaceId::new(),
            crate::workspace::WorkspaceName::parse("brain").unwrap(),
            Path::new("/home/tester/brain"),
            "tester",
            Path::new("/home/tester"),
        )
        .unwrap();
        let p = store_path(&workspace);
        assert!(p.ends_with(".config/config.json"));
        assert_eq!(
            p.parent(),
            Some(crate::settings::config_dir(&workspace).as_path())
        );
    }
}
