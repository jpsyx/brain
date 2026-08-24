//! Line-level edits inside one agenda section body.
//!
//! Every helper here is pure and works on owned lines, so the section bodies a
//! mutation does *not* touch are never rewritten.

use std::sync::OnceLock;

use regex::Regex;

use crate::tasks::complete::{Row, field};

/// A `<n>. <rest>` top-level numbered line, used to renumber a list after a
/// removal.
fn numbered_line() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(\d+)\.\s+(.*)$").expect("static numbered-line regex"))
}

/// A suggested-order line: `<n>. [ ] <time> | <body>`. The numbered prefix and
/// the time slot are preserved when a chunked task's body is swapped.
fn suggested_line() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(\d+)\.\s+\[ \]\s+(.+?)\s+\|\s+(.+)$").expect("static suggested-line regex")
    })
}

/// Does this line reference `task_id`? Agenda lines always bold the id
/// (`**T535**`), per the `/todo` "always show ID + name" principle, so the
/// bold form is the match — a bare `T535` inside prose is not a hit.
pub(super) fn has_id(line: &str, task_id: &str) -> bool {
    line.contains(&format!("**{task_id}**"))
}

/// Rewrite numbered-list prefixes to a fresh `1..N` sequence. Blank lines,
/// sub-bullets, and prose pass through unchanged.
pub(super) fn renumber(body: &[String]) -> Vec<String> {
    let mut counter = 0;
    body.iter()
        .map(|line| {
            numbered_line().captures(line).map_or_else(
                || line.clone(),
                |captures| {
                    counter += 1;
                    format!("{counter}. {}", &captures[2])
                },
            )
        })
        .collect()
}

/// Drop every line referencing `task_id`, renumbering the survivors when the
/// section is an ordered list.
pub(super) fn drop_lines_with_id(body: &[String], task_id: &str, renumbered: bool) -> Vec<String> {
    let kept: Vec<String> = body
        .iter()
        .filter(|line| !has_id(line, task_id))
        .cloned()
        .collect();
    if renumbered { renumber(&kept) } else { kept }
}

/// ` (45m)` for a numeric `estimated_duration`, empty otherwise.
fn duration_suffix(row: &Row) -> String {
    let duration = field(row, "estimated_duration");
    let duration = duration.trim();
    if !duration.is_empty() && duration.bytes().all(|byte| byte.is_ascii_digit()) {
        format!(" ({duration}m)")
    } else {
        String::new()
    }
}

/// Swap the completed chunk's MIT-callout line for the next chunk's, so the
/// user always has exactly one actionable chunk visible. Falls back to a plain
/// drop when the next chunk is already called out.
pub(super) fn swap_chunk_in_mit(body: &[String], completed_id: &str, next: &Row) -> Vec<String> {
    let next_id = field(next, "task_id").trim().to_owned();
    if body.iter().any(|line| has_id(line, &next_id)) {
        return drop_lines_with_id(body, completed_id, false);
    }
    let replacement = format!(
        "- [ ] ❗ **{next_id}** {}{}",
        field(next, "task_name").trim(),
        duration_suffix(next)
    );
    let mut swapped = false;
    body.iter()
        .map(|line| {
            if !swapped && has_id(line, completed_id) {
                swapped = true;
                replacement.clone()
            } else {
                line.clone()
            }
        })
        .collect()
}

/// Swap the completed chunk's suggested-order line for the next chunk's,
/// keeping the `<n>. [ ] <time> | ` prefix. Falls back to a plain
/// drop-and-renumber when the next chunk is already listed, or when the
/// completed line doesn't parse as a suggested-order line.
pub(super) fn swap_chunk_in_suggested(
    body: &[String],
    completed_id: &str,
    next: &Row,
) -> Vec<String> {
    let next_id = field(next, "task_id").trim().to_owned();
    if body.iter().any(|line| has_id(line, &next_id)) {
        return drop_lines_with_id(body, completed_id, true);
    }
    let mut swapped = false;
    let mut out = Vec::with_capacity(body.len());
    for line in body {
        if !swapped && has_id(line, completed_id) {
            let Some(captures) = suggested_line().captures(line) else {
                // An unparseable line for the completed id is dropped, exactly
                // as the plain-drop path would have.
                continue;
            };
            out.push(format!(
                "{}. [ ] {} | **{next_id}** {}{}",
                &captures[1],
                &captures[2],
                field(next, "task_name").trim(),
                duration_suffix(next)
            ));
            swapped = true;
            continue;
        }
        out.push(line.clone());
    }
    if swapped {
        renumber(&out)
    } else {
        drop_lines_with_id(body, completed_id, true)
    }
}
