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
