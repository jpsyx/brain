//! The brain-directory main view's two sub-views: the fuzzy search picker and
//! the directory tree.
//!
//! These sit on [`ShellState`] because they read the picker's entries and the
//! tree's widget state together, which nothing outside the aggregate may
//! touch. A child module of `shell` can reach those private fields, so the
//! sub-view surface lives here rather than growing `shell.rs`.

use std::path::{Path, PathBuf};

use crossterm::event::KeyCode;
use ratatui::{Frame, layout::Rect};

use crate::entry::Entry;

use super::{BrainDirEffect, BrainDirView, ShellState};

impl ShellState {
    pub(crate) const fn brain_dir_view(&self) -> BrainDirView {
        self.brain_dir_view
    }

    /// Open the tree sub-view on `target`.
    ///
    /// Built from the entries the search picker is already holding, which is
    /// what carries the current search scope into the tree and what makes
    /// entering it free of disk I/O.
    pub(crate) fn show_tree(&mut self, target: &Path) {
        let brain_root = self.tree.brain_root().to_path_buf();
        let show_hidden = self.tree.show_hidden();
        self.tree =
            crate::tree::TreeView::explore(self.search.entries(), &brain_root, target, show_hidden);
        self.brain_dir_view = BrainDirView::Tree;
    }

    /// Whether the tree the current search scope would open contains `target`.
    ///
    /// A caller holding a path from outside that scope has to widen the entry
    /// set before opening the tree on it.
    pub(crate) fn scope_covers(&self, target: &Path) -> bool {
        crate::tree::root::covers(self.search.entries(), self.tree.brain_root(), target)
    }

    /// Open the tree sub-view on `target` from `entries` rather than the
    /// picker's own, for a target the current search scope does not contain.
    pub(crate) fn show_tree_from(&mut self, entries: &[Entry], target: &Path) {
        let brain_root = self.tree.brain_root().to_path_buf();
        let show_hidden = self.tree.show_hidden();
        self.tree = crate::tree::TreeView::explore(entries, &brain_root, target, show_hidden);
        self.brain_dir_view = BrainDirView::Tree;
    }

    /// Open the tree sub-view at the brain root with everything collapsed.
    ///
    /// The cursor-free way in (`Ctrl+E`): it carries nothing over from the
    /// search scope or the old selection, so `entries` has to span every
    /// bucket.
    pub(crate) fn show_tree_collapsed(&mut self, entries: &[Entry]) {
        let brain_root = self.tree.brain_root().to_path_buf();
        let show_hidden = self.tree.show_hidden();
        self.tree = crate::tree::TreeView::collapsed(entries, &brain_root, show_hidden);
        self.brain_dir_view = BrainDirView::Tree;
    }

    pub(crate) const fn show_search(&mut self) {
        self.brain_dir_view = BrainDirView::Search;
    }

    pub(crate) fn reroot_tree(&mut self, entries: &[Entry], root: &Path) {
        self.tree.reroot(entries, root);
    }

    /// Whether the tree is showing dotted names.
    pub(crate) const fn tree_show_hidden(&self) -> bool {
        self.tree.show_hidden()
    }

    /// Record the tree's hidden-files choice. The caller re-walks: the picker's
    /// entries hold no dotted names, so only a fresh walk can supply them.
    pub(crate) const fn set_tree_show_hidden(&mut self, show_hidden: bool) {
        self.tree.set_show_hidden(show_hidden);
    }

    /// The walk mode the tree's current choice asks for.
    pub(crate) const fn tree_hidden_mode(&self) -> crate::entry::Hidden {
        crate::entry::Hidden::for_show_hidden(self.tree.show_hidden())
    }

    /// Rebuild the tree in place from `entries`, keeping its root and cursor,
    /// after a walk the picker did not make.
    pub(crate) fn rebuild_tree(&mut self, entries: &[Entry]) {
        let root = self.tree.root().to_path_buf();
        self.tree.rebuild(entries, &root);
    }

    /// Move the tree to the scope the picker's entries now describe, after a
    /// rescope.
    ///
    /// A re-root rather than a rebuild: the rescope rows are palette rows and
    /// the palette opens over the tree too, so the sub-view behind it must
    /// follow the new scope instead of staying at a root the old scope chose.
    pub(crate) fn resync_tree(&mut self) {
        let root = crate::tree::root::scope_root(self.search.entries(), self.tree.brain_root());
        if root == self.tree.root() {
            // The scope did not move, so this is a refresh in place: keep the
            // cursor, and let `rebuild` drop it only if the walk lost it.
            self.tree.rebuild(self.search.entries(), &root);
        } else {
            self.tree.reroot(self.search.entries(), &root);
        }
    }

    #[cfg(test)]
    pub(crate) fn tree_root(&self) -> &Path {
        self.tree.root()
    }

    /// The tree's top-level row identifiers, so a test can see what a walk
    /// actually put on screen.
    #[cfg(test)]
    pub(crate) fn tree_identifiers(&self) -> Vec<PathBuf> {
        self.tree
            .items()
            .iter()
            .map(|item| item.identifier().clone())
            .collect()
    }

    /// Select a tree node directly. Tests need this because the widget's own
    /// navigation (`key_up`, `select_first`, ...) reads the identifier list it
    /// recorded on its last render, so it is inert until the tree has been
    /// drawn at least once.
    #[cfg(test)]
    pub(crate) fn select_tree_path(&mut self, path: &Path) {
        self.tree.state_mut().select(vec![path.to_path_buf()]);
    }

    pub(crate) fn handle_tree_input(
        &mut self,
        code: KeyCode,
        ctrl: bool,
        alt: bool,
    ) -> BrainDirEffect {
        crate::tree::input::handle_tree_input(&mut self.tree, code, ctrl, alt)
    }

    pub(crate) fn render_tree(&mut self, frame: &mut Frame, area: Rect) {
        crate::tree::view::draw_into(frame, &mut self.tree, area);
    }

    pub(crate) fn selected_tree_path(&self) -> Option<PathBuf> {
        self.tree.selected_path()
    }

    /// The highlighted tree node as palette context. The `../` row is
    /// navigation rather than an entry, so it offers nothing to act on.
    pub(crate) fn selected_tree_entry_context(&self) -> Option<crate::tui::palette::EntryContext> {
        let path = self.tree.selected_path()?;
        if self.tree.is_parent_row(&path) {
            return None;
        }
        // The tree holds paths, not entries, so "is it a file?" is a question
        // for the filesystem rather than a recorded walk flag.
        let is_file = !path.is_dir();
        Some(crate::tui::palette::EntryContext {
            filename: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            dir_reldisplay: self.tree_dir_reldisplay(&path, is_file),
            is_file,
            is_markdown: is_file && crate::open_target::is_markdown(&path),
        })
    }

    /// The selected node's directory relative to the brain root, the shape the
    /// palette's directory rows read. A directory stands for itself and a file
    /// for its parent, mirroring `open_target::finder_target`.
    fn tree_dir_reldisplay(&self, path: &Path, is_file: bool) -> String {
        let dir = if is_file {
            path.parent().unwrap_or(path)
        } else {
            path
        };
        dir.strip_prefix(self.tree.brain_root())
            .map(|relative| relative.display().to_string())
            .unwrap_or_default()
    }
}
