//! Exact-item cleanup for pipe-delimited structured task references.

use std::collections::BTreeSet;

pub(crate) fn strip_items(value: &str, removed: &BTreeSet<String>) -> String {
    value
        .split('|')
        .filter(|item| !removed.contains(item.trim()))
        .collect::<Vec<_>>()
        .join("|")
}

/// Preserve unrelated tokens and duplicates, coalescing only replacement collisions.
pub(super) fn replace_tokens(
    tokens: &[String],
    replacements: &std::collections::BTreeMap<String, String>,
    trim: bool,
) -> Vec<String> {
    let key = |value: &str| {
        if trim {
            value.trim().to_owned()
        } else {
            value.to_owned()
        }
    };
    let introduced: BTreeSet<_> = tokens
        .iter()
        .filter_map(|token| replacements.get(&key(token)))
        .collect();
    let limits: std::collections::BTreeMap<_, _> = introduced
        .iter()
        .map(|destination| {
            let existing = tokens
                .iter()
                .filter(|token| key(token) == **destination)
                .count();
            ((*destination).clone(), existing.max(1))
        })
        .collect();
    let mut seen = std::collections::BTreeMap::new();
    tokens
        .iter()
        .filter_map(|token| {
            let original = key(token);
            let value = replacements.get(&original).unwrap_or(token);
            let canonical = key(value);
            if let Some(limit) = limits.get(&canonical) {
                let count = seen.entry(canonical).or_insert(0);
                *count += 1;
                if *count > *limit {
                    return None;
                }
            }
            Some(value.clone())
        })
        .collect()
}
