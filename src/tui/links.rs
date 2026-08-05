//! Openable links for a task: the Linear issue URL (when present) plus any
//! `http(s)` URLs scraped from the task's free-text notes. The Ctrl+O open
//! action and the link-picker modal both consume [`task_links`].

use std::collections::HashSet;
use std::sync::OnceLock;

use regex::Regex;

use crate::tasks::task::Task;

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

/// Classify `task`'s `links` (as built by [`task_links`]) for labeling. A
/// lone link is the Linear issue when the task carries one (Linear is always
/// listed first), otherwise it's a notes URL.
#[must_use]
pub(crate) fn classify_links(task: &Task, links: &[Link]) -> LinkKind {
    match links.len() {
        0 => LinkKind::None,
        1 if task.has_linear() => LinkKind::SingleLinear,
        1 => LinkKind::SingleNotes,
        _ => LinkKind::Multiple,
    }
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

/// All openable links for `task`, Linear issue first (when it carries an
/// identifier and `base` derives a URL), then URLs scraped from the task's
/// detail fields — `see_also` (the `↪` reference link) followed by `notes` —
/// in document order. Duplicates (including a detail URL equal to the Linear
/// URL) are dropped so nothing is listed twice.
#[must_use]
pub(crate) fn task_links(task: &Task, base: &str) -> Vec<Link> {
    let mut links: Vec<Link> = Vec::new();
    if let Some(url) = task.linear_url(base) {
        links.push(Link {
            label: format!("Linear {}", task.linear_issue.trim()),
            url,
        });
    }
    let detail_urls = extract_urls(&task.see_also)
        .into_iter()
        .chain(extract_urls(&task.notes));
    for url in detail_urls {
        if links.iter().any(|l| l.url == url) {
            continue;
        }
        links.push(Link {
            label: url.clone(),
            url,
        });
    }
    links
}
