//! Tag display styles: an emoji + label per tag, with a tiny generic default
//! set and graceful fallback for unknown tags.
//!
//! The binary ships only a small, universal default set (`mit`, `personal`,
//! `work`); every other tag a user cares about lives in their personalization
//! store and is layered on top as an override. An unstyled tag renders as its
//! raw name, so no personal taxonomy is ever baked into the public binary.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A tag's display style: an emoji glyph and a human label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagStyle {
    pub emoji: String,
    pub label: String,
}

impl TagStyle {
    fn new(emoji: &str, label: &str) -> Self {
        Self {
            emoji: emoji.to_owned(),
            label: label.to_owned(),
        }
    }

    /// The rendered form: `"{emoji} {label}"` (e.g. `"❗ MIT"`).
    #[must_use]
    fn rendered(&self) -> String {
        format!("{} {}", self.emoji, self.label)
    }
}

/// The generic, shipped-with-the-binary tag styles. Deliberately tiny and
/// universal — anything personal belongs in a user's personalization store.
#[must_use]
pub fn default_styles() -> BTreeMap<String, TagStyle> {
    [
        ("mit", TagStyle::new("❗", "MIT")),
        ("personal", TagStyle::new("✌", "personal")),
        ("work", TagStyle::new("💼", "work")),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v))
    .collect()
}

/// The generic default tag *names* shown pre-checked in the onboarding /
/// `brain config set tags` checklist. These are the keys of `default_styles`.
#[must_use]
pub fn default_tag_names() -> Vec<String> {
    let mut names: Vec<String> = default_styles().into_keys().collect();
    names.sort_unstable();
    names
}

/// Normalize a raw tag token to a valid key, or `None` if nothing usable
/// remains.
///
/// Lowercase; spaces/dashes collapse to a single underscore; other characters
/// are dropped; leading/trailing underscores trimmed. Tags may contain `_`
/// (e.g. `needs_attention`), unlike namespaces.
#[must_use]
pub fn normalize_tag(raw: &str) -> Option<String> {
    let mut out = String::new();
    for ch in raw.trim().to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else if (ch == '_' || ch == '-' || ch.is_whitespace()) && !out.ends_with('_') {
            out.push('_');
        }
    }
    let key = out.trim_matches('_').to_owned();
    (!key.is_empty()).then_some(key)
}

/// Build the `tag_styles` map for a chosen set of tag names, preserving styling
/// where we can.
///
/// An existing user style wins, then a generic default style, then a plain new
/// style (a neutral emoji + the raw name as label). Pure.
#[must_use]
pub fn styles_from_names(
    names: &[String],
    existing: &BTreeMap<String, TagStyle>,
) -> BTreeMap<String, TagStyle> {
    let defaults = default_styles();
    names
        .iter()
        .map(|n| {
            let style = existing
                .get(n)
                .or_else(|| defaults.get(n))
                .cloned()
                .unwrap_or_else(|| TagStyle::new("🏷", n));
            (n.clone(), style)
        })
        .collect()
}

/// Resolved tag styles: the generic defaults with the user's overrides layered
/// on top (an override for a default key replaces it).
#[derive(Debug, Clone)]
pub struct TagStyles(BTreeMap<String, TagStyle>);

impl TagStyles {
    /// Build the resolved map: defaults, then user overrides win.
    #[must_use]
    pub fn with_overrides(overrides: &BTreeMap<String, TagStyle>) -> Self {
        let mut map = default_styles();
        for (k, v) in overrides {
            map.insert(k.clone(), v.clone());
        }
        Self(map)
    }

    /// Render a tag: its styled `"{emoji} {label}"` if known, else the raw tag.
    #[must_use]
    pub fn label(&self, tag: &str) -> String {
        self.0
            .get(tag)
            .map_or_else(|| tag.to_owned(), TagStyle::rendered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_styles_are_the_universal_three_only() {
        let d = default_styles();
        let mut keys: Vec<&str> = d.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["mit", "personal", "work"]);
    }

    #[test]
    fn default_styles_render_with_emoji_and_label() {
        let styles = TagStyles::with_overrides(&BTreeMap::new());
        assert_eq!(styles.label("mit"), "❗ MIT");
        assert_eq!(styles.label("personal"), "✌ personal");
        assert_eq!(styles.label("work"), "💼 work");
    }

    #[test]
    fn default_tag_names_are_the_universal_three_sorted() {
        assert_eq!(default_tag_names(), ["mit", "personal", "work"]);
    }

    #[test]
    fn normalize_tag_lowercases_and_snake_cases_allowing_underscores() {
        assert_eq!(normalize_tag("Needs Attention").as_deref(), Some("needs_attention"));
        assert_eq!(normalize_tag("  CEO  ").as_deref(), Some("ceo"));
        assert_eq!(normalize_tag("data-integration").as_deref(), Some("data_integration"));
        assert_eq!(normalize_tag("__weird__").as_deref(), Some("weird"));
        assert_eq!(normalize_tag("  "), None);
        assert_eq!(normalize_tag("!!!"), None);
    }

    #[test]
    fn styles_from_names_prefers_existing_then_default_then_new() {
        let mut existing = BTreeMap::new();
        existing.insert("ceo".to_owned(), TagStyle::new("🕴", "CEO"));
        let names = ["work".to_owned(), "ceo".to_owned(), "brandnew".to_owned()];
        let styles = styles_from_names(&names, &existing);
        assert_eq!(styles["work"], TagStyle::new("💼", "work")); // generic default
        assert_eq!(styles["ceo"], TagStyle::new("🕴", "CEO")); // existing user style kept
        assert_eq!(styles["brandnew"], TagStyle::new("🏷", "brandnew")); // fresh
        assert_eq!(styles.len(), 3); // exactly the chosen set (deselected tags dropped)
    }

    #[test]
    fn unknown_tag_falls_back_to_raw_name() {
        let styles = TagStyles::with_overrides(&BTreeMap::new());
        assert_eq!(styles.label("whatever"), "whatever");
    }

    #[test]
    fn user_override_wins_over_default() {
        let mut over = BTreeMap::new();
        over.insert("mit".to_owned(), TagStyle::new("🔥", "Most Important"));
        let styles = TagStyles::with_overrides(&over);
        assert_eq!(styles.label("mit"), "🔥 Most Important");
    }

    #[test]
    fn user_override_adds_a_personal_tag() {
        let mut over = BTreeMap::new();
        over.insert("ceo".to_owned(), TagStyle::new("🕴", "CEO"));
        let styles = TagStyles::with_overrides(&over);
        // Personal tag renders via the override...
        assert_eq!(styles.label("ceo"), "🕴 CEO");
        // ...while the generic defaults still resolve.
        assert_eq!(styles.label("work"), "💼 work");
    }
}
