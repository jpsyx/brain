//! Creating a note in the workspace's user-managed `capture/` in-basket.
//!
//! The decisions are pure and unit-tested here: what the empty-input default
//! timestamp reads as, how a raw title becomes a filename, and how a name
//! that is already taken is disambiguated. [`create`] is the thin filesystem
//! shell over them.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{Local, NaiveDateTime};

/// How the empty-input default titles a note: `2026-09-23:14-05`.
pub const TIMESTAMP_FORMAT: &str = "%Y-%m-%d:%H-%M";

/// Render `now` in the default-title format.
#[must_use]
pub fn timestamp(now: NaiveDateTime) -> String {
    now.format(TIMESTAMP_FORMAT).to_string()
}

/// This machine's local time in the default-title format.
#[must_use]
pub fn now_timestamp() -> String {
    timestamp(Local::now().naive_local())
}

/// The hint under the input line, naming the exact title an empty submission
/// would get so the user never has to guess the format.
#[must_use]
pub fn helper_text(timestamp: &str) -> String {
    format!("If left empty the note's title will default to the timestamp {timestamp}")
}

/// Turn a human title into a filename stem: lowercase, alphanumerics kept,
/// everything else collapsed into single hyphens with none left dangling.
#[must_use]
pub fn kebab_case(raw: &str) -> String {
    let mut stem = String::with_capacity(raw.len());
    for character in raw.chars() {
        if character.is_alphanumeric() {
            stem.extend(character.to_lowercase());
        } else if !stem.ends_with('-') {
            stem.push('-');
        }
    }
    stem.trim_matches('-').to_owned()
}

/// A composed note, before anything touches the filesystem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureNote {
    /// The filename without its `.md` extension.
    pub stem: String,
    /// The `# ` heading, which is the user's raw input verbatim.
    pub title: String,
    /// The whole file body.
    pub body: String,
}

impl CaptureNote {
    /// Compose the note for `raw` input, falling back to `timestamp` when the
    /// user submitted nothing. The title is always the raw text as typed; the
    /// stem is its kebab-cased form, and a title with nothing kebab-able left
    /// in it (`"???"`) borrows the timestamp's stem rather than producing a
    /// nameless file.
    #[must_use]
    pub fn compose(raw: &str, timestamp: &str) -> Self {
        let trimmed = raw.trim();
        let title = if trimmed.is_empty() {
            timestamp
        } else {
            trimmed
        };
        let mut stem = kebab_case(title);
        if stem.is_empty() {
            stem = kebab_case(timestamp);
        }
        Self {
            stem,
            title: title.to_owned(),
            body: format!("# {title}\n\n"),
        }
    }

    /// The note's filename, extension included.
    #[must_use]
    pub fn filename(&self) -> String {
        format!("{}.md", self.stem)
    }
}

/// The first stem in the `stem`, `stem-2`, `stem-3`, … series that `taken`
/// rejects, or `None` when the whole series is taken.
///
/// Creating a note must never overwrite one the user already has, so
/// exhaustion is reported rather than resolved by clobbering the original.
#[must_use]
pub fn unique_stem(stem: &str, taken: &dyn Fn(&str) -> bool) -> Option<String> {
    if !taken(stem) {
        return Some(stem.to_owned());
    }
    (2u32..u32::MAX)
        .map(|suffix| format!("{stem}-{suffix}"))
        .find(|candidate| !taken(candidate))
}

/// Write a new note into `root`'s `capture/` in-basket and return its path.
///
/// The in-basket is ensured first: it is the one directory the user manages,
/// so a workspace whose `capture/` was pruned still gets a note rather than an
/// error.
pub fn create(root: &Path, raw: &str, timestamp: &str) -> Result<PathBuf> {
    let directory = root.join(crate::workspace::CAPTURE_DIRECTORY);
    crate::workspace::ensure_capture_directory(root)?;
    let note = CaptureNote::compose(raw, timestamp);
    let stem = unique_stem(&note.stem, &|candidate: &str| {
        directory.join(format!("{candidate}.md")).exists()
    })
    .with_context(|| format!("every name based on {} is already taken", note.stem))?;
    let path = directory.join(format!("{stem}.md"));
    std::fs::write(&path, &note.body)
        .with_context(|| format!("write the capture note {}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use chrono::NaiveDate;

    use super::{CaptureNote, create, helper_text, kebab_case, timestamp, unique_stem};

    fn at(hour: u32, minute: u32) -> chrono::NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 9, 23)
            .unwrap()
            .and_hms_opt(hour, minute, 0)
            .unwrap()
    }

    #[test]
    fn the_default_title_is_the_local_timestamp_to_the_minute() {
        assert_eq!(timestamp(at(14, 5)), "2026-09-23:14-05");
    }

    #[test]
    fn the_helper_text_names_the_exact_title_an_empty_submission_would_get() {
        assert_eq!(
            helper_text("2026-09-23:14-05"),
            "If left empty the note's title will default to the timestamp 2026-09-23:14-05"
        );
    }

    #[test]
    fn kebab_casing_lowercases_and_collapses_everything_that_is_not_alphanumeric() {
        assert_eq!(kebab_case("My Note Name"), "my-note-name");
        assert_eq!(kebab_case("  Q3 / OKRs — draft!  "), "q3-okrs-draft");
        assert_eq!(kebab_case("Café Notes"), "café-notes");
    }

    #[test]
    fn a_typed_title_becomes_the_filename_kebab_cased_and_the_heading_verbatim() {
        let note = CaptureNote::compose("My Note Name", "2026-09-23:14-05");

        assert_eq!(note.filename(), "my-note-name.md");
        assert_eq!(note.title, "My Note Name");
        assert_eq!(note.body, "# My Note Name\n\n");
    }

    #[test]
    fn an_empty_submission_falls_back_to_the_timestamp_for_both_halves() {
        let note = CaptureNote::compose("   ", "2026-09-23:14-05");

        // The colon is kebab-cased out of the filename; the heading keeps the
        // timestamp exactly as the helper text promised it.
        assert_eq!(note.filename(), "2026-09-23-14-05.md");
        assert_eq!(note.body, "# 2026-09-23:14-05\n\n");
    }

    #[test]
    fn a_title_with_nothing_kebab_able_in_it_still_gets_a_filename() {
        let note = CaptureNote::compose("???", "2026-09-23:14-05");

        assert_eq!(note.filename(), "2026-09-23-14-05.md");
        assert_eq!(note.body, "# ???\n\n");
    }

    #[test]
    fn a_taken_stem_is_disambiguated_rather_than_overwritten() {
        let taken: HashSet<&str> = ["my-note", "my-note-2"].into_iter().collect();
        let is_taken = |candidate: &str| taken.contains(candidate);

        assert_eq!(
            unique_stem("my-note", &is_taken).as_deref(),
            Some("my-note-3")
        );
        assert_eq!(unique_stem("other", &is_taken).as_deref(), Some("other"));
    }

    #[test]
    fn creating_a_note_writes_it_into_the_capture_in_basket() {
        let root = tempfile::tempdir().expect("tempdir");

        let path = create(root.path(), "My Note Name", "2026-09-23:14-05").expect("create");

        assert_eq!(path, root.path().join("capture").join("my-note-name.md"));
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            "# My Note Name\n\n"
        );
    }

    #[test]
    fn creating_a_second_note_of_the_same_name_leaves_the_first_alone() {
        let root = tempfile::tempdir().expect("tempdir");
        let first = create(root.path(), "Notes", "2026-09-23:14-05").expect("first");
        std::fs::write(&first, "# Notes\n\nwhat I already wrote\n").expect("edit");

        let second = create(root.path(), "Notes", "2026-09-23:14-06").expect("second");

        assert_eq!(second, root.path().join("capture").join("notes-2.md"));
        assert_eq!(
            std::fs::read_to_string(&first).expect("read"),
            "# Notes\n\nwhat I already wrote\n"
        );
    }
}
