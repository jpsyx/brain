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
/// syncs the brain dir syncs the config too) and jpsyx has nothing to do with
/// it. The one exception is the brain-root pointer itself, which can't live
/// inside the root (see [`crate::paths`]).
#[must_use]
pub fn config_dir() -> PathBuf {
    crate::paths::brain_root_path().join(".config")
}

/// Absolute path to the JSON config store.
#[must_use]
pub fn store_path() -> PathBuf {
    config_dir().join("config.json")
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
pub(crate) fn load_map() -> Map<String, Value> {
    load_map_at(&store_path())
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

pub(super) fn save_map(map: &Map<String, Value>) -> Result<()> {
    save_map_at(&store_path(), map)
}

pub(super) fn home_dir() -> PathBuf {
    std::env::var_os("HOME").map_or_else(PathBuf::new, PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_path_is_config_json_inside_the_brain_root() {
        // config.json lives at <brain-root>/.config/config.json now, so it
        // travels with the brain. We can't safely mutate the process env here,
        // so assert the shape the resolver produces.
        let p = store_path();
        assert!(p.ends_with(".config/config.json"));
        assert_eq!(p.parent(), Some(crate::settings::config_dir().as_path()));
    }
}
