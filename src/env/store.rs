//! Default-workspace compatibility for callers that still consume the former
//! flat brain-env map. Schema-v2 reads project one record into that view and
//! writes update only the default record through the atomic registry store.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::{Map, Value};

/// Absolute path to the brain-env JSON store.
#[must_use]
pub fn env_path() -> PathBuf {
    crate::paths::machine_config_dir().join("env.json")
}

/// Read the default record as a legacy-compatible flat map. Flat input remains
/// accepted only for pre-migration compatibility. Missing or broken input is
/// an empty map so existing runtime callers stay nonfatal.
#[must_use]
pub(super) fn load_map_at(path: &Path) -> Map<String, Value> {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| {
            serde_json::from_slice::<crate::workspace::MachineRegistry>(&bytes)
                .ok()
                .and_then(|registry| compatibility_map(&registry))
                .or_else(|| {
                    serde_json::from_slice::<Value>(&bytes)
                        .ok()
                        .and_then(|v| match v {
                            Value::Object(map) => Some(map),
                            _ => None,
                        })
                })
        })
        .unwrap_or_default()
}

fn compatibility_map(registry: &crate::workspace::MachineRegistry) -> Option<Map<String, Value>> {
    let selected = registry.select(None).ok()?;
    let mut map = selected.record().env.clone();
    map.insert(
        "root".to_owned(),
        Value::String(selected.record().root.display().to_string()),
    );
    Some(map)
}

/// Read the store as a JSON object. A missing/unreadable/non-object file yields
/// an empty map — a broken env never blocks startup.
#[must_use]
pub(crate) fn load_map() -> Map<String, Value> {
    load_map_at(&env_path())
}

/// Update the schema-v2 default record through the atomic registry store.
/// Flat output remains only for a missing pre-migration registry.
pub(super) fn save_map_at(path: &Path, map: &Map<String, Value>) -> Result<()> {
    let store = crate::workspace::RegistryStore::from_path(path.to_path_buf());
    store.transaction(|transaction| -> Result<()> {
        if let Ok(mut registry) = transaction.load() {
            let canonical_name = registry.default_workspace.clone();
            let record = registry
                .workspaces
                .get_mut(&canonical_name)
                .ok_or_else(|| anyhow::anyhow!("default workspace record disappeared"))?;
            if let Some(root) = map
                .get("root")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                let home = std::env::var_os("HOME").map_or_else(PathBuf::new, PathBuf::from);
                let expanded = crate::paths::expand_tilde_with_home(root, &home);
                record.root = crate::workspace::normalize_root(&expanded, &home)
                    .map_err(|error| anyhow::anyhow!(error))?;
            }
            let mut env = map.clone();
            for reserved in ["root", "receiver_enabled", "access_mode", "access_policy"] {
                env.remove(reserved);
            }
            record.env = env;
            transaction
                .save(&registry)
                .map_err(|error| anyhow::anyhow!(error))?;
            return Ok(());
        }
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
    })
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
        assert_eq!(
            p.parent(),
            Some(crate::paths::machine_config_dir().as_path())
        );
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
