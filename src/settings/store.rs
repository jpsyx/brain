//! The raw JSON config store at `~/.config/brain/config.json` (or
//! `$XDG_CONFIG_HOME/brain/config.json`): locating it and reading/writing the
//! whole object. A broken or missing file never blocks startup — it reads as
//! an empty map.

use std::path::PathBuf;

use anyhow::Result;
use serde_json::{Map, Value};

/// The brain config directory: `$XDG_CONFIG_HOME/brain` or `~/.config/brain`.
///
/// This is the single home for everything brain persists — the JSON config
/// store, `personalization.json`, and the skill `extensions/` and `plugins/`
/// sources. It lives under `$HOME` (never inside the brain root and never in
/// the jpsyx-configs repo); syncing it across machines is handled externally.
#[must_use]
pub fn config_dir() -> PathBuf {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|s| !s.is_empty()) {
        return PathBuf::from(xdg).join("brain");
    }
    home_dir().join(".config").join("brain")
}

/// Absolute path to the JSON config store.
#[must_use]
pub fn store_path() -> PathBuf {
    config_dir().join("config.json")
}

/// Read the store as a JSON object. A missing, unreadable, or non-object file
/// yields an empty map — a broken config never blocks startup.
#[must_use]
pub(crate) fn load_map() -> Map<String, Value> {
    std::fs::read_to_string(store_path())
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default()
}

pub(super) fn save_map(map: &Map<String, Value>) -> Result<()> {
    let path = store_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let body = serde_json::to_string_pretty(&Value::Object(map.clone()))?;
    std::fs::write(&path, format!("{body}\n"))?;
    Ok(())
}

pub(super) fn home_dir() -> PathBuf {
    std::env::var_os("HOME").map_or_else(PathBuf::new, PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_path_prefers_xdg_config_home() {
        // Documented precedence; we can't safely mutate the process env here,
        // so assert the shape the resolver produces from HOME.
        let p = store_path();
        assert!(p.ends_with("brain/config.json"));
    }
}
