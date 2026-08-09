//! Every workspace member's persona, keyed by portable user ID.
//!
//! A workspace can hold several people, so `personalization.json` is a map from
//! portable user ID to that person's [`Persona`]. The one-persona schema that
//! preceded it carried no owner, so it migrates onto the local user of whichever
//! machine reads it — the only person who can truthfully claim it.
//!
//! Like every other personalization read, a missing, unreadable, or malformed
//! store parses to "no personas" rather than erroring: brain must run with no
//! personalization at all.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::persona::Persona;

/// The only keyed personas schema this release writes.
pub const PERSONAS_SCHEMA_VERSION: u32 = 2;

/// Every member's persona for one workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Personas {
    pub schema_version: u32,
    #[serde(default)]
    pub personas: BTreeMap<String, Persona>,
}

impl Default for Personas {
    fn default() -> Self {
        Self {
            schema_version: PERSONAS_SCHEMA_VERSION,
            personas: BTreeMap::new(),
        }
    }
}

impl Personas {
    /// Parse a store body, migrating the legacy single-persona schema onto
    /// `legacy_owner`. Any failure yields no personas.
    #[must_use]
    pub fn parse(text: &str, legacy_owner: &str) -> Self {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
            return Self::default();
        };
        if value.get("personas").is_some() {
            return serde_json::from_value(value).unwrap_or_default();
        }
        // Legacy: one unowned persona. An empty one is nobody's, so drop it
        // rather than handing the local user a record to be nagged about.
        let legacy = serde_json::from_value::<Persona>(value).unwrap_or_default();
        let mut personas = Self::default();
        if !legacy.is_empty() {
            personas.set(legacy_owner, legacy);
        }
        personas
    }

    /// Serialize the keyed schema as the pretty JSON the store writes.
    ///
    /// # Errors
    ///
    /// Returns the `serde_json` error when the value cannot be serialized.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Every user ID with an entry, in stable ID order.
    #[must_use]
    pub fn ids(&self) -> Vec<String> {
        self.personas.keys().cloned().collect()
    }

    /// One user's stored persona, if they have an entry.
    #[must_use]
    pub fn get(&self, user_id: &str) -> Option<&Persona> {
        self.personas.get(user_id)
    }

    /// One user's persona, or an empty one when they have no entry — reading a
    /// persona never fails, it is simply unset.
    #[must_use]
    pub fn persona_of(&self, user_id: &str) -> Persona {
        self.get(user_id).cloned().unwrap_or_default()
    }

    /// Replace one user's persona, leaving every other entry untouched.
    pub fn set(&mut self, user_id: &str, persona: Persona) {
        self.personas.insert(user_id.to_owned(), persona);
    }

    /// Whether nobody has been personalized yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.personas.values().all(Persona::is_empty)
    }

    /// Which of `roster` still have nothing filled in, in stable ID order.
    ///
    /// An entry that exists but is empty counts as missing: what matters is
    /// whether a skill reading this store learns anything about the person.
    #[must_use]
    pub fn missing(&self, roster: &[&str]) -> Vec<String> {
        let mut missing = roster
            .iter()
            .filter(|id| self.get(id).is_none_or(Persona::is_empty))
            .map(|id| (*id).to_owned())
            .collect::<Vec<_>>();
        missing.sort();
        missing.dedup();
        missing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_keyed_store_parses_every_persona() {
        let personas = Personas::parse(
            r#"{
                "schema_version": 2,
                "personas": {
                    "pablo": {"name": "Pablo", "role": "CEO", "works_for": "Avandar"},
                    "sam":   {"name": "Sam", "role": "designer"}
                }
            }"#,
            "pablo",
        );

        assert_eq!(personas.ids(), ["pablo", "sam"]);
        assert_eq!(personas.get("pablo").unwrap().role, "CEO");
        assert_eq!(personas.get("sam").unwrap().name, "Sam");
    }

    #[test]
    fn a_legacy_single_persona_file_migrates_onto_its_reader() {
        // The one-persona schema had no owner, so the only user who can claim it
        // is the local person whose machine is reading it.
        let personas = Personas::parse(
            r#"{"name": "Pablo", "role": "CEO", "works_for": "Avandar",
                "namespaces": ["avandar"],
                "tag_styles": {"ceo": {"emoji": "🕴", "label": "CEO"}}}"#,
            "pablo",
        );

        assert_eq!(personas.ids(), ["pablo"]);
        let pablo = personas.get("pablo").expect("migrated persona");
        assert_eq!(pablo.role, "CEO");
        assert_eq!(pablo.namespaces, ["avandar"]);
        assert_eq!(pablo.tag_styles.get("ceo").unwrap().label, "CEO");
    }

    #[test]
    fn an_empty_legacy_file_migrates_to_no_personas_at_all() {
        // Nothing was personalized, so nobody gets an empty record they would
        // then have to be told to fill in.
        assert!(Personas::parse("{}", "pablo").is_empty());
        assert!(Personas::parse("", "pablo").is_empty());
        assert!(Personas::parse("not json", "pablo").is_empty());
    }

    #[test]
    fn a_keyed_store_is_never_reinterpreted_as_a_legacy_persona() {
        let personas = Personas::parse(r#"{"schema_version": 2, "personas": {}}"#, "pablo");

        assert!(personas.is_empty());
        assert!(personas.get("pablo").is_none());
    }

    #[test]
    fn setting_a_persona_replaces_only_that_users_entry() {
        let mut personas = Personas::parse(
            r#"{"schema_version": 2, "personas": {"pablo": {"role": "CEO"}, "sam": {"role": "designer"}}}"#,
            "pablo",
        );

        personas.set(
            "pablo",
            Persona {
                role: "founder".to_owned(),
                ..Persona::default()
            },
        );

        assert_eq!(personas.get("pablo").unwrap().role, "founder");
        assert_eq!(personas.get("sam").unwrap().role, "designer");
    }

    #[test]
    fn a_user_with_no_entry_reads_as_an_empty_persona_not_an_error() {
        let personas = Personas::parse(r#"{"schema_version": 2, "personas": {}}"#, "pablo");

        assert!(personas.persona_of("sam").is_empty());
    }

    #[test]
    fn the_store_round_trips_through_the_keyed_schema() {
        let mut personas = Personas::default();
        personas.set(
            "sam",
            Persona {
                name: "Sam".to_owned(),
                role: "designer".to_owned(),
                ..Persona::default()
            },
        );

        let json = personas.to_json().expect("serialize");
        // A legacy reader must not mistake this for a single persona, and a new
        // reader must not need the owner fallback.
        assert!(json.contains("\"schema_version\": 2"), "{json}");
        assert_eq!(Personas::parse(&json, "pablo"), personas);
    }

    #[test]
    fn users_without_a_persona_are_reported_against_the_workspace_roster() {
        let personas = Personas::parse(
            r#"{"schema_version": 2, "personas": {"pablo": {"role": "CEO"}, "sam": {}}}"#,
            "pablo",
        );

        // `sam` has an entry, but an empty one: still unpersonalized.
        assert_eq!(personas.missing(&["pablo", "sam", "alex"]), ["alex", "sam"]);
        assert!(personas.missing(&["pablo"]).is_empty());
    }
}
