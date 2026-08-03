//! Locating and reading/writing the personalization store.
//!
//! Personalization is just another brain config, so it lives at
//! `personalization.json` in the brain config dir (`<brain-root>/.config/`)
//! alongside `config.json` — **inside** the brain root, so it travels with the
//! brain. A missing or broken file reads as the default value; it never blocks
//! startup.

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
fn store_path(workspace: &crate::workspace::WorkspaceContext) -> PathBuf {
    path_in_config_dir(&crate::settings::config_dir(workspace))
}

/// Read the personalization store. Any failure (missing file, unreadable,
/// invalid JSON) yields the default value.
#[must_use]
pub fn load(workspace: &crate::workspace::WorkspaceContext) -> Personalization {
    std::fs::read_to_string(store_path(workspace))
        .map(|t| Personalization::parse(&t))
        .unwrap_or_default()
}

/// Persist the personalization store, creating the config dir if needed.
pub fn save(workspace: &crate::workspace::WorkspaceContext, p: &Personalization) -> Result<()> {
    let path = store_path(workspace);
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
        assert!(p.ends_with(".config/personalization.json"));
        assert_eq!(
            p.parent(),
            Some(crate::settings::config_dir(&workspace).as_path())
        );
    }
}
