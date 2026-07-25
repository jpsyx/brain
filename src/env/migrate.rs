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

/// Run the one-time migration. Idempotent; swallows IO errors (never fatal).
pub fn migrate() {
    let env_map = super::load_map();
    let env_has_root = env_map
        .get("root")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|s| !s.trim().is_empty());
    let env_has_md_pdf = env_map
        .get("markdown_to_pdf_path")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|s| !s.trim().is_empty());

    let legacy_pointer = std::fs::read_to_string(brain_root_pointer_path())
        .ok()
        .and_then(|s| crate::paths::parse_brain_root_file(&s));

    let config_md_pdf = crate::settings::config_get("markdown_to_pdf_path");

    let p = plan(env_has_root, legacy_pointer.as_deref(), env_has_md_pdf, config_md_pdf.as_deref());

    if let Some(root) = p.set_root {
        let _ = super::set("root", &root);
    }
    if let Some(md) = p.set_md_pdf {
        let _ = super::set("markdown_to_pdf_path", &md);
    }
    if p.clear_config_md_pdf {
        let _ = crate::settings::config_remove("markdown_to_pdf_path");
    }
}

/// `$XDG_CONFIG_HOME/brain-root` or `~/.config/brain-root` (legacy pointer).
fn brain_root_pointer_path() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|s| !s.is_empty())
        .map_or_else(
            || std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"),
            std::path::PathBuf::from,
        );
    base.join("brain-root")
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
}
