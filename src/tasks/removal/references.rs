//! Exact-item cleanup for pipe-delimited structured task references.

use std::collections::BTreeSet;

pub(crate) fn strip_items(value: &str, removed: &BTreeSet<String>) -> String {
    value
        .split('|')
        .filter(|item| !removed.contains(item.trim()))
        .collect::<Vec<_>>()
        .join("|")
}
