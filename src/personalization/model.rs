//! The personalization schema: identity facts plus tag-style overrides.
//!
//! Every field is optional and defaults to empty, so a missing or broken store
//! parses to a fully-default value rather than erroring — the app must run fine
//! with no personalization at all.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::tags::TagStyle;

/// Content-about-you, stored at `<brain-root>/.config/personalization.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Personalization {
    /// Optional display name.
    #[serde(default)]
    pub name: String,
    /// Free-text role the assistant is serving (e.g. "CEO", "engineer").
    #[serde(default)]
    pub role: String,
    /// Org the user works for, "myself", or empty.
    #[serde(default)]
    pub works_for: String,
    /// Per-tag display overrides layered over the generic defaults.
    #[serde(default)]
    pub tag_styles: BTreeMap<String, TagStyle>,
}

impl Personalization {
    /// Parse from a JSON body. Empty/blank/invalid input yields the default
    /// value — a broken store never blocks startup.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        serde_json::from_str(text).unwrap_or_default()
    }

    /// True when nothing has been personalized yet (drives first-run onboarding).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.name.is_empty()
            && self.role.is_empty()
            && self.works_for.is_empty()
            && self.tag_styles.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_parses_to_default() {
        assert!(Personalization::parse("").is_empty());
        assert!(Personalization::parse("{}").is_empty());
    }

    #[test]
    fn invalid_json_parses_to_default() {
        assert!(Personalization::parse("not json").is_empty());
    }

    #[test]
    fn full_object_parses_all_fields() {
        let p = Personalization::parse(
            r#"{
                "name": "Pablo",
                "role": "CEO",
                "works_for": "Avandar",
                "tag_styles": { "ceo": { "emoji": "🕴", "label": "CEO" } }
            }"#,
        );
        assert_eq!(p.name, "Pablo");
        assert_eq!(p.role, "CEO");
        assert_eq!(p.works_for, "Avandar");
        assert_eq!(p.tag_styles.get("ceo").unwrap().label, "CEO");
        assert!(!p.is_empty());
    }

    #[test]
    fn missing_fields_default_without_erroring() {
        // Only `role` present — the rest default, no error.
        let p = Personalization::parse(r#"{"role": "student"}"#);
        assert_eq!(p.role, "student");
        assert_eq!(p.name, "");
        assert!(p.tag_styles.is_empty());
    }

    #[test]
    fn round_trips_through_json() {
        let mut p = Personalization {
            name: "A".to_owned(),
            role: "B".to_owned(),
            works_for: "C".to_owned(),
            tag_styles: BTreeMap::new(),
        };
        p.tag_styles.insert(
            "x".to_owned(),
            TagStyle {
                emoji: "⭐".to_owned(),
                label: "star".to_owned(),
            },
        );
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(Personalization::parse(&json), p);
    }
}
