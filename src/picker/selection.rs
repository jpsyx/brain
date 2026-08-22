//! Everything about the *highlighted* entry: the path/filename/directory
//! accessors that feed the contextual palette rows and confirmations, the
//! shell-owned palette construction data, and the pure bucket-relative path
//! helpers.

use std::path::PathBuf;

use crate::menu;
use crate::open_target;
use crate::tui::palette::{CommandPalette, PaletteControls};

use super::App;

impl App {
    pub(crate) fn search_palette(
        &self,
        side: crate::state::PanelSide,
        include_msg: bool,
        receiver_enabled: bool,
    ) -> menu::SearchPalette {
        let targets = menu::Targets {
            receiver_enabled: Some(receiver_enabled),
            pdf: self.selected_markdown_filename(),
            open_file: self.selected_file_filename(),
            open_dir: self.selected_dir_reldisplay(),
            delete: self.selected_filename(),
        };
        CommandPalette::new(
            "Command palette",
            None,
            menu::items(side, include_msg, &targets),
            PaletteControls::SEARCH,
        )
    }

    /// The absolute path of the highlighted entry when it is a markdown file,
    /// else `None`. Drives the contextual "Create PDF" row and `Ctrl-G`.
    pub(crate) fn selected_markdown_path(&self) -> Option<PathBuf> {
        let path = self.selected_path()?;
        open_target::is_markdown(&path).then_some(path)
    }

    /// The filename (not the full path) of the highlighted markdown entry, for
    /// the palette row label.
    pub(crate) fn selected_markdown_filename(&self) -> Option<String> {
        self.selected_markdown_path()?
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
    }

    /// The filename (not the full path) of the highlighted entry, of any kind,
    /// for the contextual "Delete '…'" palette row.
    pub(crate) fn selected_filename(&self) -> Option<String> {
        self.selected_path()?
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
    }

    /// The filename of the highlighted entry when it is a **file**, for the
    /// contextual "Open file '…'" palette row. `None` for a directory (there's
    /// no file to open) so the row is suppressed.
    pub(crate) fn selected_file_filename(&self) -> Option<String> {
        let path = self.selected_path()?;
        if !path.is_file() {
            return None;
        }
        path.file_name().map(|n| n.to_string_lossy().into_owned())
    }

    /// The highlighted entry's directory as a bucket-relative display path
    /// (e.g. `projects/foo`), for the contextual "Open directory '…'" palette
    /// row. A file resolves to its parent directory; a directory resolves to
    /// itself (mirroring `open_target::finder_target`).
    pub(crate) fn selected_dir_reldisplay(&self) -> Option<String> {
        let m = self.matches.get(self.selected)?;
        let entry = &self.entries[m.entry_idx];
        let category = entry.bucket.label().to_ascii_lowercase();
        let rel = bucket_relative(&entry.display, &category)?;
        Some(if entry.path.is_dir() {
            rel
        } else {
            parent_reldisplay(&rel)
        })
    }

    /// Build the "Create PDF" confirmation data for shell ownership.
    pub(crate) fn pdf_confirmation(path: PathBuf) -> crate::confirm::Confirm {
        crate::confirm::Confirm::pdf(path)
    }

    /// Build the red "Delete" confirmation data for shell ownership.
    pub(crate) fn delete_confirmation(path: PathBuf) -> crate::confirm::Confirm {
        crate::confirm::Confirm::delete(path)
    }

    pub(crate) fn selected_path(&self) -> Option<PathBuf> {
        self.matches
            .get(self.selected)
            .map(|m| self.entries[m.entry_idx].path.clone())
    }
}

/// Slice an entry's `~/brain/...` display path down to its bucket-relative
/// form, starting at the `category` segment (the lowercase bucket dir name),
/// e.g. `~/brain/projects/foo/note.md` + `projects` → `projects/foo/note.md`.
///
/// Matches the first path segment equal to `category` (the top-level bucket
/// dir sits right under the brain root, so a later same-named subdirectory
/// can't shadow it). `None` if the category segment isn't present.
fn bucket_relative(display: &str, category: &str) -> Option<String> {
    let idx = display.split('/').position(|seg| seg == category)?;
    Some(display.split('/').skip(idx).collect::<Vec<_>>().join("/"))
}

/// Drop the last segment of a bucket-relative path to get its parent
/// directory, e.g. `projects/foo/note.md` → `projects/foo`. A single-segment
/// path (an entry directly under a bucket root) is returned unchanged — its
/// directory *is* the bucket.
fn parent_reldisplay(rel: &str) -> String {
    rel.rsplit_once('/')
        .map_or_else(|| rel.to_owned(), |(head, _)| head.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{Bucket, Entry};

    fn entry(bucket: Bucket, display: &str) -> Entry {
        Entry {
            path: PathBuf::from(display.replace('~', "/Users/x")),
            display: display.to_owned(),
            bucket,
        }
    }

    // --- bucket-relative directory paths (Open directory row) -----------

    #[test]
    fn bucket_relative_starts_at_the_category_segment() {
        assert_eq!(
            bucket_relative("~/brain/projects/foo/note.md", "projects").as_deref(),
            Some("projects/foo/note.md")
        );
        assert_eq!(
            bucket_relative("~/brain/resources/rust/borrow.md", "resources").as_deref(),
            Some("resources/rust/borrow.md")
        );
    }

    #[test]
    fn bucket_relative_matches_the_top_level_bucket_not_a_namesake_subdir() {
        // A later segment sharing the category name doesn't shadow the
        // top-level bucket (position finds the first match).
        assert_eq!(
            bucket_relative("~/brain/projects/projects/deep.md", "projects").as_deref(),
            Some("projects/projects/deep.md")
        );
    }

    #[test]
    fn bucket_relative_is_none_without_the_category() {
        assert_eq!(bucket_relative("~/somewhere/else/x.md", "projects"), None);
    }

    #[test]
    fn parent_reldisplay_drops_the_last_segment() {
        assert_eq!(parent_reldisplay("projects/foo/note.md"), "projects/foo");
        // A file directly under the bucket root → the bucket itself.
        assert_eq!(parent_reldisplay("projects/note.md"), "projects");
        // A lone segment (already the bucket root) is returned unchanged.
        assert_eq!(parent_reldisplay("projects"), "projects");
    }

    // --- markdown selection (Create PDF source) ------------------------

    #[test]
    fn selected_markdown_path_tracks_only_markdown_entries() {
        let entries = vec![
            entry(Bucket::Projects, "~/brain/projects/foo/plan.md"),
            entry(Bucket::Resources, "~/brain/resources/scan.pdf"),
        ];
        let mut app = App::new(&entries, "");
        // First entry is markdown.
        assert!(app.selected_markdown_path().is_some());
        assert_eq!(app.selected_markdown_filename().as_deref(), Some("plan.md"));
        // Move to the .pdf entry → not markdown.
        app.move_down();
        assert!(app.selected_markdown_path().is_none());
        assert!(app.selected_markdown_filename().is_none());
    }
}
