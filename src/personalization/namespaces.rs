//! Project namespaces: the `<namespace>__<outcome>` life-buckets.
//!
//! The binary ships a tiny generic default set; the user's real set lives in
//! personalization and is chosen via the onboarding / `brain config set`
//! checklist. Namespace slugs are lowercase-kebab with no `_`/`__` (the double
//! underscore is the project-slug separator, so it can't appear in a namespace).

/// The generic default namespaces shown pre-checked in onboarding.
#[must_use]
pub fn default_namespaces() -> Vec<String> {
    ["work", "personal"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect()
}

/// Normalize a raw namespace token to a valid slug, or `None` if nothing usable
/// remains.
///
/// Lowercase; spaces/underscores/dashes collapse to a single dash; other
/// characters are dropped; leading/trailing dashes trimmed. A namespace never
/// contains `_` because `__` is the project-slug separator.
#[must_use]
pub fn normalize(raw: &str) -> Option<String> {
    let mut out = String::new();
    for ch in raw.trim().to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else if (ch == '-' || ch == '_' || ch.is_whitespace()) && !out.ends_with('-') {
            out.push('-');
        }
    }
    let slug = out.trim_matches('-').to_owned();
    (!slug.is_empty()).then_some(slug)
}

/// The effective namespace set: the configured list, or the generic defaults
/// when the user hasn't set any.
#[must_use]
pub fn effective(configured: &[String]) -> Vec<String> {
    if configured.is_empty() {
        default_namespaces()
    } else {
        configured.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_set_is_work_and_personal() {
        assert_eq!(default_namespaces(), ["work", "personal"]);
    }

    #[test]
    fn normalize_lowercases_and_kebabs() {
        assert_eq!(normalize("Side Project").as_deref(), Some("side-project"));
        assert_eq!(normalize("  POLE  ").as_deref(), Some("pole"));
        assert_eq!(normalize("my_cool_thing").as_deref(), Some("my-cool-thing"));
    }

    #[test]
    fn normalize_collapses_and_trims_separators_and_drops_junk() {
        assert_eq!(normalize("a   b").as_deref(), Some("a-b"));
        assert_eq!(normalize("--hi--").as_deref(), Some("hi"));
        assert_eq!(normalize("caf\u{e9}!!work").as_deref(), Some("cafwork"));
    }

    #[test]
    fn normalize_rejects_empty_and_pure_junk() {
        assert_eq!(normalize(""), None);
        assert_eq!(normalize("   "), None);
        assert_eq!(normalize("!!!"), None);
    }

    #[test]
    fn effective_falls_back_to_defaults_when_unset() {
        assert_eq!(effective(&[]), default_namespaces());
        let mine = vec!["avandar".to_owned(), "personal".to_owned()];
        assert_eq!(effective(&mine), mine);
    }
}
