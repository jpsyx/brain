//! Pure `notes.md` scanning for the resources reindex.
//!
//! Derives the three notes-dependent columns of `zotero-lookup.csv`:
//! `has_summary`, `has_other_notes`, and `annotation_count`. All logic here is
//! pure text-in / flags-out so it can be unit-tested without the filesystem.

/// Flags derived from a resource's colocated `notes.md`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NotesFlags {
    pub has_summary: bool,
    pub has_other_notes: bool,
    pub annotation_count: usize,
}

impl NotesFlags {
    /// The value used when a resource has no `notes.md` at all.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            has_summary: false,
            has_other_notes: false,
            annotation_count: 0,
        }
    }
}

/// Scan a `notes.md` body into its derived flags.
#[must_use]
pub fn scan_notes(text: &str) -> NotesFlags {
    NotesFlags {
        has_summary: section_is_nonempty(text, "Summary"),
        has_other_notes: section_is_nonempty(text, "Notes"),
        annotation_count: count_annotations(&section_body(text, "Annotations")),
    }
}

/// The body lines of the `## <name>` section: everything after that exact
/// level-2 heading up to the next level-2 heading (or EOF). Level-3+ headings
/// inside the section are kept as body.
fn section_body(text: &str, name: &str) -> String {
    let heading = format!("## {name}");
    let mut in_section = false;
    let mut body = String::new();
    for line in text.lines() {
        if is_level2_heading(line) {
            if in_section {
                break;
            }
            in_section = line.trim_end() == heading;
            continue;
        }
        if in_section {
            body.push_str(line);
            body.push('\n');
        }
    }
    body
}

fn is_level2_heading(line: &str) -> bool {
    line.starts_with("## ") && !line.starts_with("### ")
}

/// A section counts as non-empty when it has content beyond blank lines and
/// beyond a lone italic "*No … attached*" placeholder sentinel.
fn section_is_nonempty(text: &str, name: &str) -> bool {
    section_body(text, name)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .any(|l| !is_placeholder(l))
}

fn is_placeholder(line: &str) -> bool {
    // The `(none)` sentinel and the italic "*No … attached.*" sentinels. The
    // italic wrapper is what distinguishes a sentinel from a real note that
    // merely begins with "No " (e.g. "No local attachment in Zotero — …").
    line.eq_ignore_ascii_case("(none)") || (line.starts_with("*No ") && line.ends_with('*'))
}

/// Count distinct blockquote blocks (maximal runs of `>`-prefixed lines) plus
/// each `*(ink annotation)*` marker line, under the `## Annotations` section.
fn count_annotations(body: &str) -> usize {
    let mut count = 0;
    let mut in_quote = false;
    for line in body.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('>') {
            if !in_quote {
                count += 1;
                in_quote = true;
            }
        } else {
            in_quote = false;
            if line.contains("*(ink annotation)*") {
                count += 1;
            }
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_with_real_content_is_nonempty_notes_placeholder_is_empty() {
        let md = "# Title\n\n## Summary\n\n*Summary based on the abstract.*\n\n\
                  ### Executive summary\n\nReal content here.\n\n\
                  ## Notes\n\n*No standalone user notes attached.*\n\n\
                  ## Annotations\n\n*No manual annotations attached.*\n";
        let flags = scan_notes(md);
        assert_eq!(
            flags,
            NotesFlags {
                has_summary: true,
                has_other_notes: false,
                annotation_count: 0,
            }
        );
    }

    #[test]
    fn counts_each_blank_separated_blockquote_block_once() {
        let md = "## Annotations\n\n> first block line one\n> still first block\n\n\
                  > second block\n\n> third block\n";
        assert_eq!(scan_notes(md).annotation_count, 3);
    }

    #[test]
    fn counts_ink_annotation_marker_lines() {
        let md = "## Annotations\n\n> a quoted block\n\n*(ink annotation)*\n\n*(ink annotation)*\n";
        assert_eq!(scan_notes(md).annotation_count, 3);
    }

    #[test]
    fn other_notes_with_real_prose_is_nonempty() {
        let md = "## Notes\n\nA genuine standalone note the user wrote.\n";
        assert!(scan_notes(md).has_other_notes);
    }

    #[test]
    fn paren_none_sentinel_counts_as_empty() {
        assert!(!scan_notes("## Notes\n\n(none)\n").has_other_notes);
    }

    #[test]
    fn all_italic_no_attached_sentinels_count_as_empty() {
        for body in [
            "*No standalone notes attached.*",
            "*No parent notes in Zotero.*",
            "*No standalone Zotero notes attached.*",
        ] {
            assert!(
                !scan_notes(&format!("## Notes\n\n{body}\n")).has_other_notes,
                "{body:?} should read as empty"
            );
        }
    }

    #[test]
    fn a_real_note_that_merely_starts_with_no_is_not_a_placeholder() {
        // Plain-text (not italic-wrapped) genuine note — must NOT be dropped.
        let md = "## Notes\n\nNo local attachment in Zotero — consult the URL directly.\n";
        assert!(scan_notes(md).has_other_notes);
    }

    #[test]
    fn annotations_section_stops_at_next_heading() {
        let md = "## Annotations\n\n> only this one\n\n## Something Else\n\n> not counted\n";
        assert_eq!(scan_notes(md).annotation_count, 1);
    }

    #[test]
    fn missing_sections_are_empty() {
        let flags = scan_notes("# Just a title\n\nsome text\n");
        assert_eq!(flags, NotesFlags::empty());
    }
}
