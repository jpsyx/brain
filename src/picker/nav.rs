//! Query edits and cursor movement: the text mutations that trigger a
//! `refilter`, the up/down/page/jump navigation over the match list, and the
//! `ensure_visible` scroll logic that keeps the cursor (and its section
//! header) on screen.

use super::{App, DisplayRow};

impl App {
    const PAGE_SIZE: usize = 10;

    // -- query mutations (pub(crate) for the embedded search panel) -------

    pub(crate) fn push_query(&mut self, c: char) {
        self.query.push(c);
        self.refilter();
    }

    pub(crate) fn pop_query(&mut self) {
        self.query.pop();
        self.refilter();
    }

    pub(crate) fn clear_query(&mut self) {
        self.query.clear();
        self.refilter();
    }

    pub(crate) fn delete_word(&mut self) {
        let cut = self
            .query
            .trim_end()
            .rfind(char::is_whitespace)
            .map_or(0, |i| i + 1);
        self.query.truncate(cut);
        self.refilter();
    }

    pub(crate) const fn jump_first(&mut self) {
        self.selected = 0;
    }

    pub(crate) fn jump_last(&mut self) {
        self.selected = self.matches.len().saturating_sub(1);
    }

    pub(crate) const fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub(crate) fn move_down(&mut self) {
        if self.selected + 1 < self.matches.len() {
            self.selected += 1;
        }
    }

    pub(crate) const fn page_up(&mut self) {
        self.selected = self.selected.saturating_sub(Self::PAGE_SIZE);
    }

    pub(crate) fn page_down(&mut self) {
        let max = self.matches.len().saturating_sub(1);
        self.selected = self.selected.saturating_add(Self::PAGE_SIZE).min(max);
    }

    /// Returns the display-row index of the currently-selected match.
    fn selected_row(&self) -> Option<usize> {
        self.display_rows
            .iter()
            .position(|r| matches!(r, DisplayRow::Match(i) if *i == self.selected))
    }

    pub(super) fn ensure_visible(&mut self, height: usize) {
        if height == 0 {
            return;
        }
        let Some(sel_row) = self.selected_row() else {
            return;
        };
        // If the section header sits directly above the selected match,
        // anchor the top to the header so it stays in view.
        let header_above = sel_row > 0
            && matches!(
                self.display_rows.get(sel_row - 1),
                Some(DisplayRow::Header(_, _))
            );
        let upper_anchor = if header_above { sel_row - 1 } else { sel_row };

        if upper_anchor < self.top {
            self.top = upper_anchor;
        } else if sel_row >= self.top + height {
            self.top = sel_row + 1 - height;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{Bucket, Entry};
    use std::path::PathBuf;

    fn entry(bucket: Bucket, display: &str) -> Entry {
        Entry {
            path: PathBuf::from(display.replace('~', "/Users/x")),
            display: display.to_owned(),
            bucket,
        }
    }

    fn sample() -> Vec<Entry> {
        vec![
            entry(Bucket::Projects, "~/brain/projects/ann-afloat/plan.md"),
            entry(Bucket::Projects, "~/brain/projects/zebra/notes.md"),
            entry(Bucket::Areas, "~/brain/areas/health/log.md"),
            entry(Bucket::Resources, "~/brain/resources/rust/borrow.md"),
        ]
    }

    #[test]
    fn move_down_then_up_clamps_at_bounds() {
        let entries = sample();
        let mut app = App::new(&entries, "");
        assert_eq!(app.selected, 0);
        app.move_up(); // already at top
        assert_eq!(app.selected, 0);
        app.move_down();
        app.move_down();
        assert_eq!(app.selected, 2);
        // Walk to the end and try to overshoot.
        app.move_down();
        app.move_down();
        assert_eq!(app.selected, 3);
    }

    #[test]
    fn page_down_and_up_saturate() {
        let entries = sample();
        let mut app = App::new(&entries, "");
        app.page_down();
        assert_eq!(app.selected, app.matches.len() - 1);
        app.page_up();
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn selected_path_tracks_the_cursor() {
        let entries = sample();
        let mut app = App::new(&entries, "");
        app.move_down(); // index 1 → second projects entry
        assert_eq!(app.selected_path().unwrap(), entries[1].path);
    }

    #[test]
    fn selected_path_is_none_when_empty() {
        let entries: Vec<Entry> = Vec::new();
        let app = App::new(&entries, "");
        assert!(app.selected_path().is_none());
    }
}
