//! One-time, idempotent migration from flat brain env into the schema-v2
//! workspace registry. The top-level wrapper remains nonfatal at startup.

use anyhow::Context;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LegacySource {
    Registry,
    RootPointer,
    DefaultRoot,
    Fresh,
}

const fn classify_legacy_source(
    registry_exists: bool,
    pointer_exists: bool,
    default_root_exists: bool,
) -> LegacySource {
    if registry_exists {
        LegacySource::Registry
    } else if pointer_exists {
        LegacySource::RootPointer
    } else if default_root_exists {
        LegacySource::DefaultRoot
    } else {
        LegacySource::Fresh
    }
}

/// Whether create/attach must preserve an existing legacy installation before
/// establishing another workspace record.
pub(crate) fn registry_setup_needs_migration() -> anyhow::Result<bool> {
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("HOME is not set"))?;
    let config_home = config_home();
    let paths = [
        config_home.join("brain/env.json"),
        config_home.join("brain-root"),
        home.join("brain"),
    ];
    let mut exists = [false; 3];
    for (found, path) in exists.iter_mut().zip(&paths) {
        *found = path
            .try_exists()
            .with_context(|| format!("inspect legacy Brain path {}", path.display()))?;
    }
    Ok(classify_legacy_source(exists[0], exists[1], exists[2]) != LegacySource::Fresh)
}

/// Run the one-time migration against the real machine dirs. Idempotent;
/// swallows IO errors (never fatal).
pub fn migrate() {
    let _ = migrate_checked();
}

/// Run the real migration and surface any storage or validation failure.
pub(crate) fn migrate_checked() -> anyhow::Result<()> {
    let home =
        std::env::var_os("HOME").map_or_else(std::path::PathBuf::new, std::path::PathBuf::from);
    migrate_in_with_home(&home, &config_home(), &crate::settings::config_dir())
}

fn migrate_in_with_home(
    home: &std::path::Path,
    config_home: &std::path::Path,
    brain_config_dir: &std::path::Path,
) -> anyhow::Result<()> {
    let env_json_path = config_home.join("brain").join("env.json");
    let legacy_body = std::fs::read(&env_json_path).unwrap_or_default();
    let config_json_path = brain_config_dir.join("config.json");
    let mut config_map = crate::settings::load_map_at(&config_json_path);
    let fallback_env = config_map
        .get("markdown_to_pdf_path")
        .cloned()
        .map(|value| serde_json::Map::from_iter([("markdown_to_pdf_path".to_owned(), value)]))
        .unwrap_or_default();

    let outcome = crate::workspace::registry::migrate_legacy_with(
        home,
        &config_home.join("brain"),
        &legacy_body,
        &fallback_env,
    )?;
    let markdown_was_persisted = outcome
        .registry
        .select(None)
        .ok()
        .is_some_and(|selected| selected.record().env.contains_key("markdown_to_pdf_path"));
    if markdown_was_persisted && config_map.remove("markdown_to_pdf_path").is_some() {
        crate::settings::save_map_at(&config_json_path, &config_map)?;
    }
    Ok(())
}

/// `$XDG_CONFIG_HOME`, or `~/.config` (the real machine dirs `migrate()` uses).
fn config_home() -> std::path::PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|s| !s.is_empty())
        .map_or_else(
            || {
                std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                    .join(".config")
            },
            std::path::PathBuf::from,
        )
}

#[cfg(test)]
mod tests {
    use super::{LegacySource, classify_legacy_source};

    #[test]
    fn legacy_source_classifier_distinguishes_fresh_and_all_migration_evidence() {
        assert_eq!(
            classify_legacy_source(false, false, false),
            LegacySource::Fresh
        );
        assert_eq!(
            classify_legacy_source(true, false, false),
            LegacySource::Registry
        );
        assert_eq!(
            classify_legacy_source(false, true, false),
            LegacySource::RootPointer
        );
        assert_eq!(
            classify_legacy_source(false, false, true),
            LegacySource::DefaultRoot
        );
        assert_eq!(
            classify_legacy_source(true, true, true),
            LegacySource::Registry
        );
    }
}
