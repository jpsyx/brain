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
use crate::tui::search_view::all_bucket_roots;
use crate::tui::state::{SearchEffect, ShellState};

pub(crate) fn handle_tree_view_key(
    shell: &mut ShellState,
    k: &KeyEvent,
    ctrl: bool,
    alt: bool,
) -> SearchEffect {
    shell.handle_tree_input(k.code, ctrl, alt)
}

impl App {
    /// Open the tree sub-view on `target`. Exploring is also a palette command
    /// reachable from the tasks view, so it brings the brain-directory view
    /// along with it rather than switching a view nobody is looking at.
    pub(crate) fn explore_entry(&mut self, target: &Path) {
        self.shell.show_tree(target);
        self.shell.show_main_view(MainView::BrainSearch);
    }

    /// Move the tree's root and rebuild it there (the `../` row).
    pub(crate) fn reroot_tree(&mut self, root: &Path) {
        let entries = self.brain_dir_entries();
        self.shell.reroot_tree(&entries, root);
    }

    /// The full bucket set. Only a re-root needs this: it widens past the
    /// current scope, so the entries the picker holds are not enough.
    fn brain_dir_entries(&self) -> Vec<Entry> {
        let root = self.context.workspace_root();
        crate::entry::collect(root, &all_bucket_roots(root)).unwrap_or_default()
    }
}
