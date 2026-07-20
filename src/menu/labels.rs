//! Pure label builders for the palette's *contextual* rows (the ones that
//! carry a filename or directory path). Each row elides an overlong name so a
//! single entry can't stretch the content-sized modal past its width budget.

/// The most characters of a *filename* we show in a contextual palette row
/// (`Create PDF for '…'`, `Open file '…'`, `Delete '…'`) before eliding. Caps
/// how far a single name can stretch the (content-sized, see `palette_width`)
/// modal. Shared by those rows so they elide identically.
const LABEL_MAX_FILENAME: usize = 24;

/// The most characters of a *directory path* we show in the `Open dir '…'` row
/// before middle-eliding. A touch wider than a filename because a path packs in
/// more meaning per char (category + trailing segments); the shorter `Open dir`
/// prefix keeps the row from growing the modal despite the extra budget.
const LABEL_MAX_DIR: usize = 26;

/// Shorten a filename to fit a palette row: a head, an ellipsis, and a tail
/// that is always the **full extension** (e.g. `…mp4`, never `…p4`), so the
/// file type stays legible. Names without a usable extension keep the last two
/// chars, e.g. `really-long-note-name-here.md` → `really-long-note-h...md`.
fn truncate_label_filename(name: &str, max: usize) -> String {
    const ELLIPSIS: &str = "...";
    const DEFAULT_TAIL: usize = 2;
    let count = name.chars().count();
    if count <= max {
        return name.to_owned();
    }
    // Keep the whole extension as the tail, as long as a non-empty head still
    // fits after it; otherwise fall back to the last two chars.
    let tail_len = file_extension(name)
        .map(|ext| ext.chars().count())
        .filter(|&ext| ext + ELLIPSIS.len() < max)
        .unwrap_or(DEFAULT_TAIL)
        .max(DEFAULT_TAIL);
    let head_len = max.saturating_sub(ELLIPSIS.len() + tail_len);
    let head: String = name.chars().take(head_len).collect();
    let tail: String = name.chars().skip(count - tail_len).collect();
    format!("{head}{ELLIPSIS}{tail}")
}

/// The file extension (the chars after the last `.`), or `None` for a name with
/// no dot, a leading-dot dotfile (`.bashrc`), or a trailing dot (`name.`).
fn file_extension(name: &str) -> Option<&str> {
    name.rfind('.')
        .filter(|&dot| dot > 0)
        .map(|dot| &name[dot + 1..])
        .filter(|ext| !ext.is_empty())
}

/// Shorten a bucket-relative directory path to fit a palette row. Unlike a
/// filename (elided head + tail), a path keeps its leading **category**
/// segment (`projects`/`areas`/`resources`/`archive`) and drops the *middle*,
/// so the tail — the parts nearest the entry — stays readable, e.g.
/// `resources/a/b/c/final/parts` → `resources/.../final/parts`.
///
/// When over `max`, the head is `<category>/...` and the tail is as many of
/// the path's trailing chars as fit (pure char-count, so a cut can land
/// mid-segment). A single-segment path that overflows falls back to the
/// filename-style head+tail elision.
fn truncate_label_dir(rel: &str, max: usize) -> String {
    const MID: &str = "/...";
    if rel.chars().count() <= max {
        return rel.to_owned();
    }
    let Some(slash) = rel.find('/') else {
        return truncate_label_filename(rel, max);
    };
    let category = &rel[..slash];
    // `rest` leads with '/', so `<category>` + `/...` + `<rest tail>` reads as
    // `category/.../tail` when the tail happens to start at a separator.
    let rest = &rel[slash..];
    let prefix = category.chars().count() + MID.chars().count();
    let budget = max.saturating_sub(prefix);
    let rest_count = rest.chars().count();
    let tail: String = rest.chars().skip(rest_count.saturating_sub(budget)).collect();
    format!("{category}{MID}{tail}")
}

/// The "Create PDF" row label for a given markdown filename, with the
/// filename elided if it would overflow the palette row.
#[must_use]
pub fn create_pdf_label(filename: &str) -> String {
    format!(
        "Create PDF for '{}'",
        truncate_label_filename(filename, LABEL_MAX_FILENAME)
    )
}

/// The "Open file" row label for a given filename, elided with the same
/// threshold and head+tail logic as the "Create PDF" row.
#[must_use]
pub fn open_file_label(filename: &str) -> String {
    format!(
        "Open file '{}'",
        truncate_label_filename(filename, LABEL_MAX_FILENAME)
    )
}

/// The "Open dir" row label for a bucket-relative directory path, elided with
/// the middle-ellipsis path logic.
#[must_use]
pub fn open_dir_label(rel_dir: &str) -> String {
    format!("Open dir '{}'", truncate_label_dir(rel_dir, LABEL_MAX_DIR))
}

/// The "Delete" row label for a given filename, elided with the same
/// threshold as the "Create PDF" row so the two contextual rows line up.
#[must_use]
pub fn delete_label(filename: &str) -> String {
    format!(
        "Delete '{}'",
        truncate_label_filename(filename, LABEL_MAX_FILENAME)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_file_label_elides_long_names_like_create_pdf() {
        let label = open_file_label("really-long-note-name-that-overflows.md");
        assert!(label.starts_with("Open file 'really-long"), "got: {label}");
        assert!(label.contains("..."), "got: {label}");
        let shown = label
            .trim_start_matches("Open file '")
            .trim_end_matches('\'');
        assert_eq!(shown.chars().count(), LABEL_MAX_FILENAME);
    }

    #[test]
    fn open_dir_label_shows_a_short_path_in_full() {
        assert_eq!(
            open_dir_label("projects/foo/bar"),
            "Open dir 'projects/foo/bar'"
        );
    }

    #[test]
    fn open_dir_label_keeps_category_and_middle_elides_a_long_path() {
        // Over the threshold: the leading category survives, the middle is
        // dropped, and the trailing chars fill the remaining budget.
        let rel = "resources/aa/bb/cc/dd/final/parts/of/path";
        let label = open_dir_label(rel);
        let shown = label
            .trim_start_matches("Open dir '")
            .trim_end_matches('\'')
            .to_owned();
        assert!(shown.starts_with("resources/..."), "got: {shown}");
        assert!(shown.ends_with("path"), "got: {shown}");
        assert_eq!(shown.chars().count(), LABEL_MAX_DIR);
    }

    #[test]
    fn open_dir_label_uses_the_short_open_dir_prefix() {
        assert!(
            open_dir_label("projects/foo").starts_with("Open dir '"),
            "the row is labeled 'Open dir', not 'Open directory'"
        );
    }

    #[test]
    fn open_dir_label_budget_is_wider_than_a_filename() {
        // The directory path gets more room (26) than an elided filename (24).
        assert_eq!(LABEL_MAX_DIR, 26);
        let rel = "resources/aa/bb/cc/dd/eee/final/parts/of/path";
        let shown = open_dir_label(rel)
            .trim_start_matches("Open dir '")
            .trim_end_matches('\'')
            .to_owned();
        assert_eq!(shown.chars().count(), LABEL_MAX_DIR);
    }

    #[test]
    fn short_filenames_are_shown_in_full() {
        assert_eq!(create_pdf_label("plan.md"), "Create PDF for 'plan.md'");
    }

    #[test]
    fn long_filenames_are_elided_with_a_trailing_md() {
        let label = create_pdf_label("really-long-note-name-that-overflows.md");
        assert!(label.starts_with("Create PDF for 'really-long"), "got: {label}");
        assert!(label.contains("..."), "got: {label}");
        assert!(label.ends_with("md'"), "got: {label}");
        // The shown filename is capped at LABEL_MAX_FILENAME chars.
        let shown = label
            .trim_start_matches("Create PDF for '")
            .trim_end_matches('\'');
        assert_eq!(shown.chars().count(), LABEL_MAX_FILENAME);
    }

    #[test]
    fn truncation_keeps_head_ellipsis_and_two_tail_chars() {
        // Deterministic shape: 19-char head + "..." + "md" = 24.
        assert_eq!(
            truncate_label_filename("abcdefghijklmnopqrstuvwxyz.md", 24),
            "abcdefghijklmnopqrs...md"
        );
    }

    #[test]
    fn truncation_always_keeps_the_full_extension() {
        // A 3-char extension survives whole — `mp4`, never `p4`.
        let label = truncate_label_filename("a-really-long-clip-name-here.mp4", 24);
        assert!(label.ends_with("...mp4"), "got: {label}");
        assert_eq!(label.chars().count(), 24);

        // A longer extension (`webp`) is still shown in full.
        let webp = truncate_label_filename("some-long-screenshot-name.webp", 24);
        assert!(webp.ends_with("...webp"), "got: {webp}");

        // No extension → the previous two-char tail behavior.
        let bare = truncate_label_filename("a-really-long-name-without-ext", 24);
        assert!(bare.contains("..."), "got: {bare}");
        assert_eq!(bare.chars().count(), 24);
    }

    #[test]
    fn delete_label_shares_the_create_pdf_ellipsis_threshold() {
        let name = "really-long-note-name-that-overflows.md";
        let shown = delete_label(name)
            .trim_start_matches("Delete '")
            .trim_end_matches('\'')
            .to_owned();
        assert!(shown.contains("..."), "got: {shown}");
        // Same cap as the Create PDF row (LABEL_MAX_FILENAME).
        assert_eq!(shown.chars().count(), LABEL_MAX_FILENAME);
    }
}
