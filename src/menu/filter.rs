//! The palette's substring filter. Typing narrows the row list; each row's
//! matchable text includes its 1-based number, so a digit *or* any label word
//! finds the row. Pure, so it's unit-testable without a TUI.

use super::model::Choice;

/// The string a row is matched against: its 1-based number plus its label,
/// e.g. `"6. Search resources"`. Including the number makes both `6` and any
/// label word find the row.
fn matchable_text(index: usize, label: &str) -> String {
    format!("{}. {label}", index + 1)
}

/// Substring filter mirroring the picker's word-atom semantics: every
/// whitespace-separated word in `query` must appear (case-insensitively)
/// somewhere in the row's `matchable_text`. An empty query matches all rows.
fn item_matches(query: &str, index: usize, label: &str) -> bool {
    let haystack = matchable_text(index, label).to_lowercase();
    query
        .split_whitespace()
        .all(|word| haystack.contains(&word.to_lowercase()))
}

/// Indices into `rows` that match `query`, in menu order.
pub(super) fn filter_indices(rows: &[(Choice, String)], query: &str) -> Vec<usize> {
    rows.iter()
        .enumerate()
        .filter(|(i, (_, label))| item_matches(query, *i, label))
        .map(|(i, _)| i)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::model::{Targets, items};
    use crate::state::PanelSide;

    fn rows() -> Vec<(Choice, String)> {
        items(PanelSide::Right, true, &Targets::default())
    }

    #[test]
    fn matchable_text_includes_the_one_based_number() {
        assert_eq!(matchable_text(0, "Message brain"), "1. Message brain");
        assert_eq!(matchable_text(7, "Global search"), "8. Global search");
    }

    #[test]
    fn empty_query_matches_every_row() {
        let r = rows();
        assert_eq!(filter_indices(&r, ""), (0..r.len()).collect::<Vec<_>>());
    }

    #[test]
    fn digit_query_matches_the_row_with_that_number() {
        let r = rows();
        // "7" is matchable because the number is part of the row's text.
        // Row 7 is "Global search" now that the dropped "Go to root" row no
        // longer sits between "Open tasks" and the searches.
        let hits = filter_indices(&r, "7");
        assert_eq!(hits, vec![6]);
        assert_eq!(r[hits[0]].0, Choice::GlobalSearch);
    }

    #[test]
    fn archive_row_is_searchable_by_label() {
        let r = rows();
        let hits = filter_indices(&r, "archive");
        assert_eq!(hits.len(), 1);
        assert_eq!(r[hits[0]].0, Choice::SearchArchive);
    }

    #[test]
    fn layout_row_is_searchable_by_label() {
        let r = rows();
        let hits = filter_indices(&r, "move brain panel");
        assert_eq!(hits.len(), 1);
        assert_eq!(r[hits[0]].0, Choice::ToggleLayout);
    }

    #[test]
    fn query_is_case_insensitive() {
        let r = rows();
        assert_eq!(filter_indices(&r, "MESSAGE"), filter_indices(&r, "message"));
        assert!(!filter_indices(&r, "MESSAGE").is_empty());
    }

    #[test]
    fn every_word_must_match() {
        let r = rows();
        // "search projects" both appear only in the Search projects row.
        let hits = filter_indices(&r, "search projects");
        assert_eq!(hits.len(), 1);
        assert_eq!(r[hits[0]].0, Choice::SearchProjects);
    }

    #[test]
    fn unmatched_query_yields_no_rows() {
        let r = rows();
        assert!(filter_indices(&r, "nonexistentxyz").is_empty());
    }
}
