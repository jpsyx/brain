//! The brain-directory tree sub-view's pure model.
//!
//! [`TreeView`] pairs the widget's own `TreeState` with the root the tree is
//! currently showing and the items built for it. Every decision about *what*
//! the tree contains lives in `build` and `root`; this type only holds the
//! result and hands the selection back as a path.

pub(crate) mod build;
pub(crate) mod input;
pub(crate) mod root;
pub(crate) mod view;

use std::path::{Path, PathBuf};

use tui_tree_widget::{TreeItem, TreeState};

use crate::entry::Entry;

/// The tree sub-view's state: where it is rooted, what it is showing, and
/// where the cursor sits.
pub(crate) struct TreeView {
    root: PathBuf,
    brain_root: PathBuf,
    items: Vec<TreeItem<'static, PathBuf>>,
    state: TreeState<PathBuf>,
    /// The directory the synthetic `../` row re-roots to, when there is one.
    /// Held so a selection can be told apart from a real entry by identity
    /// rather than by label.
    parent_row: Option<PathBuf>,
}

impl TreeView {
    /// An empty tree rooted at the brain root, for a shell that has not opened
    /// the sub-view yet.
    pub(crate) fn empty(brain_root: &Path) -> Self {
        Self {
            root: brain_root.to_path_buf(),
            brain_root: brain_root.to_path_buf(),
            items: Vec::new(),
            state: TreeState::default(),
            parent_row: None,
        }
    }

    /// Open the tree on `target`: rooted at the scope the entries describe,
    /// expanded along the target's ancestors, with the target selected.
    pub(crate) fn explore(entries: &[Entry], brain_root: &Path, target: &Path) -> Self {
        let root = root::scope_root(entries, brain_root);
        let mut view = Self::empty(brain_root);
        view.rebuild(entries, &root);
        for opened in build::opened_for(target, &view.root) {
            view.state.open(opened);
        }
        let identifier = build::identifier_path(target, &view.root);
        if !identifier.is_empty() {
            view.state.select(identifier);
        }
        view
    }

    /// Move the root to `root` and rebuild from `entries`, keeping nothing: a
    /// re-root is a different tree, and carrying an old selection into it
    /// would point at a node that may no longer be there.
    pub(crate) fn reroot(&mut self, entries: &[Entry], root: &Path) {
        self.state = TreeState::default();
        self.rebuild(entries, root);
    }

    /// Rebuild the items for `root`, keeping the cursor where it is, for a
    /// refresh in place.
    ///
    /// A selection the new entry set no longer renders is dropped: after a
    /// delete the cursor would otherwise still name the trashed path, and both
    /// `Enter` and every palette entry row would act on it.
    pub(crate) fn rebuild(&mut self, entries: &[Entry], root: &Path) {
        self.root = root.to_path_buf();
        self.items = build::build_items(entries, root, &self.brain_root);
        self.parent_row = root::ascend(root, &self.brain_root);
        let stale = self
            .selected_path()
            .is_some_and(|selected| !self.renders(entries, &selected));
        if stale {
            self.state.select(Vec::new());
        }
    }

    /// Whether `path` still names a row the tree draws: the `../` row, an
    /// entry, or one of the directories synthesized above an entry.
    fn renders(&self, entries: &[Entry], path: &Path) -> bool {
        self.is_parent_row(path)
            || entries
                .iter()
                .any(|entry| entry.path == path || entry.path.starts_with(path))
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn brain_root(&self) -> &Path {
        &self.brain_root
    }

    pub(crate) fn items(&self) -> &[TreeItem<'static, PathBuf>] {
        &self.items
    }

    pub(crate) const fn state_mut(&mut self) -> &mut TreeState<PathBuf> {
        &mut self.state
    }

    /// The selected node's path, whether it is an entry or the `../` row.
    pub(crate) fn selected_path(&self) -> Option<PathBuf> {
        self.state.selected().last().cloned()
    }

    /// Whether `path` is the synthetic `../` row rather than a real entry.
    /// Selecting it re-roots; selecting anything else acts on a file or
    /// directory.
    pub(crate) fn is_parent_row(&self, path: &Path) -> bool {
        self.parent_row.as_deref() == Some(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::Bucket;

    fn entry(path: &str, is_dir: bool) -> Entry {
        Entry {
            path: PathBuf::from(path),
            display: path.to_owned(),
            bucket: Bucket::Projects,
            is_dir,
        }
    }

    fn entries() -> Vec<Entry> {
        vec![
            entry("/brain/projects/atlas", true),
            entry("/brain/projects/atlas/plan.md", false),
            entry("/brain/projects/loose.md", false),
        ]
    }

    /// The tree's top-level rows, which is what a rebuild replaces.
    fn identifiers(view: &TreeView) -> Vec<PathBuf> {
        view.items()
            .iter()
            .map(|item| item.identifier().clone())
            .collect()
    }

    #[test]
    fn exploring_an_entry_roots_at_the_scope_and_selects_that_entry() {
        let view = TreeView::explore(
            &entries(),
            Path::new("/brain"),
            Path::new("/brain/projects/atlas/plan.md"),
        );

        assert_eq!(view.root(), Path::new("/brain/projects"));
        assert_eq!(
            view.selected_path(),
            Some(PathBuf::from("/brain/projects/atlas/plan.md"))
        );
    }

    #[test]
    fn exploring_opens_the_ancestors_so_the_target_is_visible() {
        let view = TreeView::explore(
            &entries(),
            Path::new("/brain"),
            Path::new("/brain/projects/atlas/plan.md"),
        );

        assert!(
            view.state
                .opened()
                .contains(&vec![PathBuf::from("/brain/projects/atlas")]),
            "the target's directory must be open for it to be selectable"
        );
    }

    #[test]
    fn re_rooting_moves_the_root_rebuilds_the_items_and_drops_the_cursor() {
        let mut view = TreeView::explore(
            &entries(),
            Path::new("/brain"),
            Path::new("/brain/projects/loose.md"),
        );
        assert_eq!(view.root(), Path::new("/brain/projects"));
        assert_eq!(
            identifiers(&view),
            vec![
                PathBuf::from("/brain"),
                PathBuf::from("/brain/projects/atlas"),
                PathBuf::from("/brain/projects/loose.md"),
            ],
            "the ../ row, then the scoped bucket's own contents"
        );

        view.reroot(&entries(), Path::new("/brain"));

        assert_eq!(view.root(), Path::new("/brain"));
        assert_eq!(
            identifiers(&view),
            vec![PathBuf::from("/brain/projects")],
            "a different tree: the bucket directory, and no ../ row above it"
        );
        assert_eq!(
            view.selected_path(),
            None,
            "the old selection addressed the old root, so it cannot be carried over"
        );
    }

    #[test]
    fn a_refresh_drops_a_selection_the_new_entries_no_longer_hold() {
        // What happens after a delete: the walk comes back without the file,
        // and a cursor left on it would offer `Open 'loose.md'` for a path
        // that is gone.
        let mut view = TreeView::explore(
            &entries(),
            Path::new("/brain"),
            Path::new("/brain/projects/loose.md"),
        );
        assert_eq!(
            view.selected_path(),
            Some(PathBuf::from("/brain/projects/loose.md"))
        );

        let survivors = vec![
            entry("/brain/projects/atlas", true),
            entry("/brain/projects/atlas/plan.md", false),
        ];
        view.rebuild(&survivors, Path::new("/brain/projects"));

        assert_eq!(view.selected_path(), None);
    }

    #[test]
    fn a_refresh_keeps_a_selection_that_is_still_there() {
        let mut view = TreeView::explore(
            &entries(),
            Path::new("/brain"),
            Path::new("/brain/projects/loose.md"),
        );

        view.rebuild(&entries(), Path::new("/brain/projects"));
        assert_eq!(
            view.selected_path(),
            Some(PathBuf::from("/brain/projects/loose.md"))
        );

        // The ../ row is a row too, though no entry's path equals it.
        view.state.select(vec![PathBuf::from("/brain")]);
        view.rebuild(&entries(), Path::new("/brain/projects"));
        assert_eq!(view.selected_path(), Some(PathBuf::from("/brain")));
    }

    #[test]
    fn a_refresh_keeps_a_directory_that_only_exists_through_its_children() {
        // `entry::collect` skips each walk root, so the bucket directory has
        // no Entry of its own; it is a row only because its children nest
        // under it. Dropping it as "gone" would clear the cursor on a refresh.
        let mut view = TreeView::explore(
            &entries(),
            Path::new("/brain"),
            Path::new("/brain/projects/loose.md"),
        );
        view.reroot(&entries(), Path::new("/brain"));
        view.state.select(vec![PathBuf::from("/brain/projects")]);

        view.rebuild(&entries(), Path::new("/brain"));

        assert_eq!(view.selected_path(), Some(PathBuf::from("/brain/projects")));
    }

    #[test]
    fn the_parent_row_is_reported_as_a_re_root_not_an_entry() {
        let view = TreeView::explore(
            &entries(),
            Path::new("/brain"),
            Path::new("/brain/projects/loose.md"),
        );

        assert!(view.is_parent_row(Path::new("/brain")));
        assert!(!view.is_parent_row(Path::new("/brain/projects/loose.md")));
    }

    #[test]
    fn a_tree_at_the_brain_root_has_no_parent_row_to_confuse_an_entry_with() {
        let mut view = TreeView::explore(
            &entries(),
            Path::new("/brain"),
            Path::new("/brain/projects/loose.md"),
        );
        view.reroot(&entries(), Path::new("/brain"));

        assert!(!view.is_parent_row(Path::new("/brain")));
    }

    #[test]
    fn an_empty_tree_starts_at_the_brain_root_with_nothing_selected() {
        let view = TreeView::empty(Path::new("/brain"));

        assert_eq!(view.root(), Path::new("/brain"));
        assert_eq!(view.brain_root(), Path::new("/brain"));
        assert!(view.items().is_empty());
        assert_eq!(view.selected_path(), None);
        assert!(!view.is_parent_row(Path::new("/brain")));
    }

    #[test]
    fn state_mut_exposes_the_widget_state_for_input_handling() {
        // Nothing else calls `state_mut`: input handling (a later task) is
        // what actually drives it. Exercise it directly here so it is not
        // dead code in the interim, and so the exposed state really is the
        // same one `selected_path` reads back from.
        let mut view = TreeView::empty(Path::new("/brain"));

        view.state_mut()
            .select(vec![PathBuf::from("/brain/projects")]);

        assert_eq!(view.selected_path(), Some(PathBuf::from("/brain/projects")));
    }
}
