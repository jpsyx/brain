//! One-time, idempotent migration from flat brain env into the schema-v2
//! workspace registry. The top-level wrapper remains nonfatal at startup.

/// Run the one-time migration against the real machine dirs. Idempotent;
/// swallows IO errors (never fatal).
pub fn migrate() {
    let home =
        std::env::var_os("HOME").map_or_else(std::path::PathBuf::new, std::path::PathBuf::from);
    migrate_in_with_home(&home, &config_home(), &crate::settings::config_dir());
}

fn migrate_in_with_home(
    home: &std::path::Path,
    config_home: &std::path::Path,
    brain_config_dir: &std::path::Path,
) {
    let env_json_path = config_home.join("brain").join("env.json");
    let legacy_body = std::fs::read(&env_json_path).unwrap_or_default();
    let config_json_path = brain_config_dir.join("config.json");
    let mut config_map = crate::settings::load_map_at(&config_json_path);
    let fallback_env = config_map
        .get("markdown_to_pdf_path")
        .cloned()
        .map(|value| serde_json::Map::from_iter([("markdown_to_pdf_path".to_owned(), value)]))
        .unwrap_or_default();

    let Ok(outcome) = crate::workspace::registry::migrate_legacy_with(
        home,
        &config_home.join("brain"),
        &legacy_body,
        &fallback_env,
    ) else {
        return;
    };
    let markdown_was_persisted = outcome
        .registry
        .select(None)
        .ok()
        .is_some_and(|selected| selected.record().env.contains_key("markdown_to_pdf_path"));
    if markdown_was_persisted && config_map.remove("markdown_to_pdf_path").is_some() {
        let _ = crate::settings::save_map_at(&config_json_path, &config_map);
    }
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
