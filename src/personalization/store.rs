//! Locating and reading/writing the personalization store, a hidden
//! `.config/personalization.json` *inside the brain root*. A missing or broken
//! file reads as the default value; it never blocks startup.

use std::path::{Path, PathBuf};

use anyhow::Result;

use super::model::Personalization;

/// The store path for a given brain root: `<root>/.config/personalization.json`.
/// Pure (no IO) so it is testable without a real root.
#[must_use]
pub fn path_in_root(root: &Path) -> PathBuf {
    root.join(".config").join("personalization.json")
}

/// Resolve the store path against the configured brain root.
fn store_path() -> Result<PathBuf> {
    Ok(path_in_root(&crate::paths::brain_root()?))
}

/// Read the personalization store. Any failure (missing root, missing file,
/// unreadable, invalid JSON) yields the default value.
#[must_use]
pub fn load() -> Personalization {
    let Ok(path) = store_path() else {
        return Personalization::default();
    };
    std::fs::read_to_string(&path)
        .map(|t| Personalization::parse(&t))
        .unwrap_or_default()
}

/// Persist the personalization store, creating the hidden `.config/` dir if
/// needed.
pub fn save(p: &Personalization) -> Result<()> {
    let path = store_path()?;
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
    fn path_is_hidden_config_under_root() {
        assert_eq!(
            path_in_root(Path::new("/Users/x/brain")),
            PathBuf::from("/Users/x/brain/.config/personalization.json")
        );
    }

    #[test]
    fn path_is_dot_prefixed_so_finder_and_picker_skip_it() {
        let p = path_in_root(Path::new("/any/root"));
        // The parent dir is hidden (dot-prefixed).
        assert_eq!(p.parent().unwrap().file_name().unwrap(), ".config");
    }
}
