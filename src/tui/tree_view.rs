//! The brain-directory tree sub-view's glue.
//!
//! Mirrors `search_view`: the pure decision lives in `tree::input`, and this
//! module turns the resulting effect into an app action. The two sub-views
//! share one effect enum, so `Open`, `Reveal`, PDF, delete, refresh, palette,
//! and quit run through exactly the same code for both.

use std::path::Path;

use crossterm::event::KeyEvent;

use crate::entry::Entry;
use crate::main_view::MainView;
use crate::tui::App;
use crate::tui::modal_state::FlashKind;
use crate::tui::search_view::all_bucket_roots;
use crate::tui::state::{BrainDirEffect, ShellState};

pub(crate) fn handle_tree_view_key(
    shell: &mut ShellState,
    k: &KeyEvent,
    ctrl: bool,
    alt: bool,
) -> BrainDirEffect {
    shell.handle_tree_input(k.code, ctrl, alt)
}

impl App {
    /// Open the tree sub-view on `target`. Exploring is also a palette command
    /// reachable from the tasks view, so it brings the brain-directory view
    /// along with it rather than switching a view nobody is looking at.
    pub(crate) fn explore_entry(&mut self, target: &Path) {
        if self.shell.scope_covers(target) {
            self.shell.show_tree(target);
        } else {
            // The palette's target picker walks every bucket, so its pick can
            // sit outside the current search scope. Widen rather than open a
            // tree the target is not in.
            let entries = self.brain_dir_entries();
            self.shell.show_tree_from(&entries, target);
        }
        self.restore_hidden_rows();
        self.shell.show_main_view(MainView::BrainSearch);
    }

    /// Open the tree at the brain root, collapsed (`Ctrl+E`).
    ///
    /// Depends on no cursor, so it never carries the search scope: the brain
    /// root needs entries from every bucket, which is a walk of its own.
    pub(crate) fn open_file_explorer(&mut self) {
        let entries = self.brain_dir_entries();
        self.shell.show_tree_collapsed(&entries);
        self.shell.show_main_view(MainView::BrainSearch);
    }

    /// Flip whether the tree shows dotted names, persist the choice, and
    /// re-walk for the rows the flip needs.
    ///
    /// The search picker is deliberately untouched: it never shows hidden
    /// entries, so this is the tree's state alone.
    pub(crate) fn toggle_hidden_files(&mut self) {
        let show_hidden = !self.shell.tree_show_hidden();
        self.shell.set_tree_show_hidden(show_hidden);
        let entries = self.brain_dir_entries();
        self.shell.rebuild_tree(&entries);
        let persisted = self.persist_show_hidden_files();
        crate::logging::log(if show_hidden {
            "tree showing hidden files"
        } else {
            "tree hiding hidden files"
        });
        let verb = if show_hidden { "shown" } else { "hidden" };
        self.status.set_flash(FlashKind::Info(persisted.map_or_else(
            |error| format!("hidden files {verb} for this session only; saving failed: {error:#}"),
            |()| format!("hidden files {verb} (saved to config)"),
        )));
    }

    /// Write the live hidden-files choice to portable config.
    ///
    /// The palette row is the same decision as `brain config set
    /// show_hidden_files=…`, so it is stored the same way and survives a
    /// restart. A write failure never fails the toggle — the running session
    /// still honors it — but it is surfaced rather than silently downgrading a
    /// persistent choice to a session one.
    fn persist_show_hidden_files(&self) -> anyhow::Result<()> {
        crate::settings::set(
            self.context.workspace(),
            "show_hidden_files",
            if self.shell.tree_show_hidden() {
                "true"
            } else {
                "false"
            },
        )
    }

    /// Move the tree to the scope the picker's entries now describe, then put
    /// back the rows only a hidden-files walk can supply.
    pub(crate) fn resync_brain_dir_tree(&mut self) {
        self.shell.resync_tree();
        self.restore_hidden_rows();
    }

    /// Re-walk and rebuild when the tree is showing hidden files.
    ///
    /// A tree built from the picker's entries is missing every dotted name, so
    /// only a fresh walk can put them back. A no-op otherwise, which is the
    /// common case.
    fn restore_hidden_rows(&mut self) {
        if !self.shell.tree_show_hidden() {
            return;
        }
        let entries = self.brain_dir_entries();
        self.shell.rebuild_tree(&entries);
    }

    /// Move the tree's root and rebuild it there (the `../` row).
    pub(crate) fn reroot_tree(&mut self, root: &Path) {
        let entries = self.brain_dir_entries();
        self.shell.reroot_tree(&entries, root);
    }

    /// The full bucket set, for the paths that widen past the current scope.
    ///
    /// A re-root, the explorer, exploring a target the scope does not hold,
    /// and any walk that has to include hidden entries.
    fn brain_dir_entries(&self) -> Vec<Entry> {
        let root = self.context.workspace_root();
        crate::entry::collect_with(root, &all_bucket_roots(root), self.shell.tree_hidden_mode())
            .unwrap_or_default()
    }
}
