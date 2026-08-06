//! Legacy assignment values remapped onto existing portable users.

use std::collections::BTreeMap;

use super::UserId;

/// Raw `assigned_to` values remapped onto existing portable members.
///
/// A legacy workspace may assign work to a value that is not a portable user
/// (`me`, a first name, a retired ID). Mapping that value onto an existing
/// member rewrites the task rows instead of inventing a duplicate person.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AssignmentRewrites {
    entries: BTreeMap<String, String>,
}

impl AssignmentRewrites {
    /// Build an empty rewrite set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Remap one raw assignment value onto an existing portable user.
    pub fn record(&mut self, from: &str, to: &UserId) {
        self.entries
            .insert(from.trim().to_owned(), to.as_str().to_owned());
    }

    /// Whether no assignment value is remapped.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Rewrite one raw assignment value, preserving anything unmapped.
    #[must_use]
    pub fn apply<'a>(&'a self, value: &'a str) -> &'a str {
        self.entries.get(value.trim()).map_or(value, String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::AssignmentRewrites;
    use crate::users::UserId;

    #[test]
    fn a_recorded_value_is_matched_after_trimming_and_leaves_every_other_value_alone() {
        let mut rewrites = AssignmentRewrites::new();
        assert!(rewrites.is_empty());

        rewrites.record(" me ", &UserId::parse("pablo").unwrap());

        assert!(!rewrites.is_empty());
        assert_eq!(rewrites.apply("me"), "pablo");
        assert_eq!(rewrites.apply(" me "), "pablo");
        assert_eq!(rewrites.apply("pablo"), "pablo");
        assert_eq!(rewrites.apply("wife"), "wife");
        assert_eq!(rewrites.apply(""), "");
    }

    #[test]
    fn an_empty_rewrite_set_changes_nothing() {
        let rewrites = AssignmentRewrites::new();

        assert!(rewrites.is_empty());
        assert_eq!(rewrites.apply("me"), "me");
    }
}
