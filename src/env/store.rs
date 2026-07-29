//! The raw JSON brain-env store at `~/.config/brain/env.json`: locating it and
//! reading/writing the whole object. A broken or missing file never blocks
//! startup — it reads as an empty map. Brain env is machine-local (`root`,
//! `markdown_to_pdf_path`, the `sync` block) and is NOT Backblaze-synced.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::{Map, Value};

/// Absolute path to the brain-env JSON store.
#[must_use]
pub fn env_path() -> PathBuf {
    crate::paths::machine_config_dir().join("env.json")
}

/// Read a JSON object at an explicit `path`. A missing/unreadable/non-object
/// file yields an empty map — a broken env never blocks startup. Split out of
/// [`load_map`] so the migration (`env::migrate`) can read a store at an
/// explicit dir hermetically, without going through the real `env_path()`.
#[must_use]
pub(super) fn load_map_at(path: &Path) -> Map<String, Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default()
}

/// Read the store as a JSON object. A missing/unreadable/non-object file yields
/// an empty map — a broken env never blocks startup.
#[must_use]
pub(crate) fn load_map() -> Map<String, Value> {
    load_map_at(&env_path())
}

/// Write `map` as the JSON object at an explicit `path`, creating parent dirs
/// as needed. Split out of [`save_map`] for the same reason as
/// [`load_map_at`].
pub(super) fn save_map_at(path: &Path, map: &Map<String, Value>) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let body = serde_json::to_string_pretty(&Value::Object(map.clone()))?;
    std::fs::write(path, format!("{body}\n"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

pub(super) fn save_map(map: &Map<String, Value>) -> Result<()> {
    save_map_at(&env_path(), map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_path_is_env_json_in_the_machine_config_dir() {
        let p = env_path();
        assert!(p.ends_with("brain/env.json"));
        assert_eq!(p.parent(), Some(crate::paths::machine_config_dir().as_path()));
    }

    #[cfg(unix)]
    #[test]
    fn saved_env_store_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("env.json");
        save_map_at(&path, &Map::new()).expect("save env");
        let mode = std::fs::metadata(path)
            .expect("env metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
}
