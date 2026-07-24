//! Locating and reading/writing the personalization store.
//!
//! Personalization is just another brain config, so it lives at
//! `personalization.json` in the brain config dir (`~/.config/brain/`) alongside
//! `config.json` — **not** inside the brain root. A missing or broken file reads
//! as the default value; it never blocks startup.

use std::path::{Path, PathBuf};

use anyhow::Result;

use super::model::Personalization;

/// The store path within a config dir: `<config-dir>/personalization.json`.
/// Pure (no IO) so it is testable without touching the environment.
#[must_use]
pub fn path_in_config_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("personalization.json")
}

/// Resolve the store path against the brain config dir (`~/.config/brain`).
fn store_path() -> PathBuf {
    path_in_config_dir(&crate::settings::config_dir())
}

/// Read the personalization store. Any failure (missing file, unreadable,
/// invalid JSON) yields the default value.
#[must_use]
pub fn load() -> Personalization {
    std::fs::read_to_string(store_path())
        .map(|t| Personalization::parse(&t))
        .unwrap_or_default()
}

/// Persist the personalization store, creating the config dir if needed.
pub fn save(p: &Personalization) -> Result<()> {
    let path = store_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let body = serde_json::to_string_pretty(p)?;
    std::fs::write(&path, format!("{body}\n"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_lives_beside_config_json_in_the_brain_config_dir() {
        assert_eq!(
            path_in_config_dir(Path::new("/Users/x/.config/brain")),
            PathBuf::from("/Users/x/.config/brain/personalization.json")
        );
    }

    #[test]
    fn resolved_store_path_is_under_the_brain_config_dir() {
        // Whatever the environment, it resolves beside config.json.
        let p = store_path();
        assert!(p.ends_with("brain/personalization.json"));
        assert_eq!(p.parent(), Some(crate::settings::config_dir().as_path()));
    }
}
