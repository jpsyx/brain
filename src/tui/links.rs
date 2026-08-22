//! Link effect types plus URL extraction. `TasksState` owns the selected-task
//! link policy; the App only executes its resulting open-or-choose plan.

use std::collections::HashSet;
use std::sync::OnceLock;

use regex::Regex;

/// One openable destination. `label` is what the picker shows; `url` is
/// what gets handed to `/usr/bin/open`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Link {
    pub(crate) label: String,
    pub(crate) url: String,
}

/// What kind of link set a task has — drives the "open" command's
/// visibility (hidden only when `None`) and its label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LinkKind {
    /// No openable link at all — the command is hidden, Ctrl+O is a no-op.
    None,
    /// Exactly one link, and it is the Linear issue (no notes URLs).
    SingleLinear,
    /// Exactly one link, and it came from the notes (task has no Linear).
    SingleNotes,
    /// Two or more links — opening raises the picker.
    Multiple,
}

fn url_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Greedy run of non-space, non-bracket chars after the scheme. Trailing
    // punctuation is trimmed separately so a URL at the end of a sentence
    // doesn't swallow the period.
    RE.get_or_init(|| Regex::new(r"https?://[^\s<>()\[\]{}|\\^]+").expect("static URL regex"))
}

/// Trailing characters that are almost always sentence punctuation rather
/// than part of the URL.
fn trim_url_tail(s: &str) -> &str {
    s.trim_end_matches(['.', ',', ';', ':', '!', '?', '"', '\'', '»', '…'])
}

/// Extract `http(s)` URLs from `text` in document order, de-duplicated
/// (first occurrence wins).
#[must_use]
pub(crate) fn extract_urls(text: &str) -> Vec<String> {
    let mut seen: HashSet<&str> = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for m in url_regex().find_iter(text) {
        let url = trim_url_tail(m.as_str());
        if url.is_empty() {
            continue;
        }
        if seen.insert(url) {
            out.push(url.to_owned());
        }
    }
    out
}
