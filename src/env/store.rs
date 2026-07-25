//! The raw JSON brain-env store at `~/.config/brain/env.json`: locating it and
//! reading/writing the whole object. A broken or missing file never blocks
//! startup — it reads as an empty map. Brain env is machine-local (`root`,
//! `markdown_to_pdf_path`, the `sync` block) and is NOT Backblaze-synced.

use std::path::PathBuf;

use anyhow::Result;
use serde_json::{Map, Value};

/// Absolute path to the brain-env JSON store.
#[must_use]
pub fn env_path() -> PathBuf {
    crate::paths::machine_config_dir().join("env.json")
}

/// Read the store as a JSON object. A missing/unreadable/non-object file yields
/// an empty map — a broken env never blocks startup.
#[must_use]
pub(crate) fn load_map() -> Map<String, Value> {
    std::fs::read_to_string(env_path())
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default()
}

pub(super) fn save_map(map: &Map<String, Value>) -> Result<()> {
    let path = env_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let body = serde_json::to_string_pretty(&Value::Object(map.clone()))?;
    std::fs::write(&path, format!("{body}\n"))?;
    Ok(())
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
}
