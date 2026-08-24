//! The preservation guarantee: whatever the sync doesn't own comes back
//! byte-for-byte.

use super::{row, today};
use crate::tasks::agenda::{Action, Snapshot, sync_markdown};

fn untouched(text: &str) -> String {
    sync_markdown(
        text,
        "T999",
        Action::Done,
        &Snapshot {
            tasks: &[],
            habits: &[],
        },
        today(),
    )
}

#[test]
fn a_document_without_a_trailing_newline_keeps_it_that_way() {
    assert_eq!(
        untouched("# Agenda\n\n## Cut order\n\n1. **T1** Ship"),
        "# Agenda\n\n## Cut order\n\n1. **T1** Ship"
    );
}

#[test]
fn a_document_with_no_sections_round_trips() {
    let text = "# Agenda\n\n**Load:** nothing\n";
    assert_eq!(untouched(text), text);
}

#[test]
fn sub_headings_stay_inside_their_parent_section() {
    let text = "\
## Suggested order

### Morning

1. **T1** Ship it
2. **T2** Then this
";
    let tasks = [row(&[("task_id", "T1"), ("task_name", "Ship it")])];
    let out = sync_markdown(
        text,
        "T1",
        Action::Done,
        &Snapshot {
            tasks: &tasks,
            habits: &[],
        },
        today(),
    );
    // The `###` line is body content, so it survives and does not get
    // renumbered; only the numbered list is resequenced.
    assert_eq!(
        out,
        "## Suggested order\n\n### Morning\n\n1. **T2** Then this\n"
    );
}

#[test]
fn an_unbolded_id_mention_is_not_a_match() {
    let text = "## Cut order\n\n1. **T2** Follow up on T1\n";
    let tasks = [row(&[("task_id", "T1"), ("task_name", "Ship it")])];
    let out = sync_markdown(
        text,
        "T1",
        Action::Done,
        &Snapshot {
            tasks: &tasks,
            habits: &[],
        },
        today(),
    );
    assert_eq!(out, text);
}

#[test]
fn a_re_derived_section_lands_before_appended_optional_content() {
    let text = "\
## Suggested order

1. **T1** Ship it

## Appendix <!-- brain:optional-content -->

Whatever the agenda's author appended.
";
    let tasks = [row(&[
        ("task_id", "T1"),
        ("task_name", "Ship it"),
        ("status", "done"),
        ("completed_date", "2026-08-24"),
    ])];
    let out = sync_markdown(
        text,
        "T1",
        Action::Done,
        &Snapshot {
            tasks: &tasks,
            habits: &[],
        },
        today(),
    );
    let headings: Vec<&str> = out.lines().filter(|line| line.starts_with("## ")).collect();
    assert_eq!(
        headings,
        [
            "## Suggested order",
            "## ✅ Completed today",
            "## Appendix <!-- brain:optional-content -->",
        ]
    );
    assert!(
        out.contains("Whatever the agenda's author appended."),
        "{out}"
    );
}
