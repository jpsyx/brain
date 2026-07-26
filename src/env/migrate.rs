//! One-time, idempotent migration into brain env: fold the legacy
//! `~/.config/brain-root` pointer into the `root` key, and relocate
//! `markdown_to_pdf_path` from brain config (`config.json`) into brain env.
//! Never fatal — a failed migration must not block startup.

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Plan {
    /// Write this value into the env `root` key (from the legacy pointer).
    pub(super) set_root: Option<String>,
    /// Write this value into the env `markdown_to_pdf_path` (from brain config).
    pub(super) set_md_pdf: Option<String>,
    /// Remove `markdown_to_pdf_path` from the brain-config store after moving it.
    pub(super) clear_config_md_pdf: bool,
}

/// Decide the migration plan. Pure: no IO.
pub(super) fn plan(
    env_has_root: bool,
    legacy_pointer: Option<&str>,
    env_has_md_pdf: bool,
    config_md_pdf: Option<&str>,
) -> Plan {
    let non_empty = |s: &str| -> Option<String> {
        let t = s.trim();
        (!t.is_empty()).then(|| t.to_owned())
    };
    Plan {
        set_root: (!env_has_root).then_some(legacy_pointer).flatten().and_then(non_empty),
        set_md_pdf: (!env_has_md_pdf).then_some(config_md_pdf).flatten().and_then(non_empty),
        clear_config_md_pdf: config_md_pdf.is_some(),
    }
}

/// Run the one-time migration against the real machine dirs. Idempotent;
/// swallows IO errors (never fatal).
pub fn migrate() {
    migrate_in(&config_home(), &crate::settings::config_dir());
}

/// The migration IO against explicit dirs (hermetically testable — no reads of
/// the process's real `$HOME`/`$XDG_CONFIG_HOME`).
///
/// `config_home` is `$XDG_CONFIG_HOME`, or `~/.config` — home to both the
/// legacy `<config_home>/brain-root` pointer file and the
/// `<config_home>/brain/env.json` store this migration writes into.
/// `brain_config_dir` is the **brain-root** config directory
/// (`<root>/.config`) holding `config.json`, from which
/// `markdown_to_pdf_path` is relocated. Swallows IO errors (never fatal).
pub(crate) fn migrate_in(config_home: &std::path::Path, brain_config_dir: &std::path::Path) {
    let env_json_path = config_home.join("brain").join("env.json");
    let pointer_path = config_home.join("brain-root");
    let config_json_path = brain_config_dir.join("config.json");

    let env_map = super::store::load_map_at(&env_json_path);
    let env_has_root = env_map
        .get("root")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|s| !s.trim().is_empty());
    let env_has_md_pdf = env_map
        .get("markdown_to_pdf_path")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|s| !s.trim().is_empty());

    let legacy_pointer = std::fs::read_to_string(&pointer_path)
        .ok()
        .and_then(|s| crate::paths::parse_brain_root_file(&s));

    let config_map = crate::settings::load_map_at(&config_json_path);
    let config_md_pdf =
        config_map.get("markdown_to_pdf_path").and_then(serde_json::Value::as_str).map(str::to_owned);

    let p = plan(env_has_root, legacy_pointer.as_deref(), env_has_md_pdf, config_md_pdf.as_deref());

    if p.set_root.is_some() || p.set_md_pdf.is_some() {
        let mut m = env_map;
        if let Some(root) = p.set_root {
            m.insert("root".to_owned(), serde_json::Value::from(root));
        }
        if let Some(md) = p.set_md_pdf {
            m.insert("markdown_to_pdf_path".to_owned(), serde_json::Value::from(md));
        }
        let _ = super::store::save_map_at(&env_json_path, &m);
    }

    if p.clear_config_md_pdf {
        let mut m = config_map;
        if m.remove("markdown_to_pdf_path").is_some() {
            let _ = crate::settings::save_map_at(&config_json_path, &m);
        }
    }
}

/// `$XDG_CONFIG_HOME`, or `~/.config` (the real machine dirs `migrate()` uses).
fn config_home() -> std::path::PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").filter(|s| !s.is_empty()).map_or_else(
        || std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"),
        std::path::PathBuf::from,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_pointer_into_root_only_when_env_root_missing() {
        assert_eq!(plan(false, Some("/srv/brain"), true, None).set_root, Some("/srv/brain".to_owned()));
        assert_eq!(plan(true, Some("/srv/brain"), true, None).set_root, None);
        assert_eq!(plan(false, Some("  "), true, None).set_root, None);
        assert_eq!(plan(false, None, true, None).set_root, None);
    }

    #[test]
    fn relocates_md_pdf_only_when_env_lacks_it_and_config_has_it() {
        let p = plan(true, None, false, Some("/opt/mdpdf"));
        assert_eq!(p.set_md_pdf, Some("/opt/mdpdf".to_owned()));
        assert!(p.clear_config_md_pdf);
        let p = plan(true, None, true, Some("/opt/mdpdf"));
        assert_eq!(p.set_md_pdf, None);
        assert!(p.clear_config_md_pdf);
        assert!(!plan(true, None, false, None).clear_config_md_pdf);
    }

    // --- migrate_in: end-to-end file-IO wiring (hermetic — explicit dirs, no
    // process env var reads or mutation). These prove the wiring around the
    // pure `plan()` above (already covered by the two tests above); no
    // `std::env::set_var`/`remove_var` is used anywhere here.

    fn read_json(path: &std::path::Path) -> serde_json::Value {
        let body = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("expected {} to exist: {e}", path.display()));
        serde_json::from_str(&body).expect("valid json")
    }

    #[test]
    fn migrate_in_folds_pointer_into_env_root() {
        let config_home = tempfile::tempdir().expect("tempdir");
        let brain_config_dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(config_home.path().join("brain-root"), "~/brain\n").expect("write pointer");

        migrate_in(config_home.path(), brain_config_dir.path());

        let env_json = read_json(&config_home.path().join("brain/env.json"));
        assert_eq!(env_json.get("root").and_then(serde_json::Value::as_str), Some("~/brain"));
    }

    #[test]
    fn migrate_in_relocates_markdown_to_pdf_path() {
        let config_home = tempfile::tempdir().expect("tempdir");
        let brain_config_dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            brain_config_dir.path().join("config.json"),
            r#"{"markdown_to_pdf_path": "/opt/mdpdf/bin/md-to-pdf"}"#,
        )
        .expect("write config.json");

        migrate_in(config_home.path(), brain_config_dir.path());

        let env_json = read_json(&config_home.path().join("brain/env.json"));
        assert_eq!(
            env_json.get("markdown_to_pdf_path").and_then(serde_json::Value::as_str),
            Some("/opt/mdpdf/bin/md-to-pdf")
        );
        // C1's real behavior: the stale key is cleared out of config.json once
        // it has been relocated into brain env.
        let config_json = read_json(&brain_config_dir.path().join("config.json"));
        assert!(config_json.get("markdown_to_pdf_path").is_none());
    }

    #[test]
    fn migrate_in_never_overrides_an_existing_env_root_and_is_idempotent() {
        let config_home = tempfile::tempdir().expect("tempdir");
        let brain_config_dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(config_home.path().join("brain-root"), "/legacy/root").expect("write pointer");
        std::fs::create_dir_all(config_home.path().join("brain")).expect("mkdir");
        std::fs::write(config_home.path().join("brain/env.json"), r#"{"root": "/current/root"}"#)
            .expect("write env.json");

        migrate_in(config_home.path(), brain_config_dir.path());
        let after_first = read_json(&config_home.path().join("brain/env.json"));
        assert_eq!(after_first.get("root").and_then(serde_json::Value::as_str), Some("/current/root"));

        // Running again changes nothing further (idempotent).
        migrate_in(config_home.path(), brain_config_dir.path());
        let after_second = read_json(&config_home.path().join("brain/env.json"));
        assert_eq!(after_first, after_second);
    }
}
