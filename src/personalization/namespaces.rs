//! Project namespaces: the `<namespace>__<outcome>` life-buckets.
//!
//! The binary ships a tiny generic default set; the user's real set lives in
//! personalization and is chosen via the onboarding / `brain config set`
//! checklist. Namespace slugs are lowercase-kebab with no `_`/`__` (the double
//! underscore is the project-slug separator, so it can't appear in a namespace).

/// The generic default namespaces shown pre-checked in onboarding.
#[must_use]
pub fn default_namespaces() -> Vec<String> {
    ["work", "personal"].iter().map(|s| (*s).to_owned()).collect()
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
    fn effective_falls_back_to_defaults_when_unset() {
        assert_eq!(effective(&[]), default_namespaces());
        let mine = vec!["avandar".to_owned(), "personal".to_owned()];
        assert_eq!(effective(&mine), mine);
    }
}
