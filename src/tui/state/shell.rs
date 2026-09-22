use std::path::{Path, PathBuf};

use crossterm::event::KeyCode;
use ratatui::{Frame, layout::Rect};

use crate::entry::Entry;
use crate::main_view::{Dir, MainView, StartupDestination};
use crate::state::PanelSide;
use crate::tui::logs_view::LogsView;
use crate::tui::model::{BrainTab, Panel, SessionTabId};

pub(crate) struct ShellState {
    main_view: MainView,
    focus: Panel,
    panel_side: PanelSide,
    brain_rect: Option<Rect>,
    search: crate::picker::App,
    brain_dir_view: BrainDirView,
    tree: crate::tree::TreeView,
    logs_view: Option<LogsView>,
    active_brain_tab: BrainTab,
    /// Set by the palette's "Quit brain" row. The event loop reads and clears
    /// it after every keystroke, so a command leaves the shell through the same
    /// door as the `Ctrl+Q` chord.
    quit_requested: bool,
}

/// The brain-directory main view's effect enum, covering **both** sub-views
/// (fuzzy search and the directory tree). `Open`/`Reveal`/`Quit`/
/// `OpenPalette`/`Refresh` are shared rather than duplicated into a parallel
/// `TreeEffect`, since both sub-views act on the same picked path and the
/// same app-level actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SearchEffect {
    None,
    Quit,
    Open(PathBuf),
    Reveal(PathBuf),
    OpenPalette,
    ConfirmPdf(PathBuf),
    Refresh,
    ConfirmDelete(PathBuf),
    /// Switch to the tree sub-view, rooted at the current scope and opened on
    /// this path.
    Explore(PathBuf),
    /// Leave the tree sub-view for the search sub-view.
    BackToSearch,
    /// Move the tree's root to this directory and rebuild.
    Reroot(PathBuf),
}

/// Which sub-view the brain-directory main view is showing.
///
/// This is an axis *inside* one main view, like the tasks view's `View`, not a
/// fourth entry in `MainView::CYCLE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BrainDirView {
    Search,
    Tree,
}

fn resolve_active_tab(requested: BrainTab, open: &[SessionTabId]) -> BrainTab {
    match requested {
        BrainTab::Session(id) if open.contains(&id) => BrainTab::Session(id),
        _ => BrainTab::Main,
    }
}

fn tab_order(open: &[SessionTabId]) -> Vec<BrainTab> {
    let mut tabs = vec![BrainTab::Main];
    tabs.extend(open.iter().copied().map(BrainTab::Session));
    tabs
}

fn tab_for_slot(slot: usize, open: &[SessionTabId]) -> Option<BrainTab> {
    tab_order(open).get(slot).copied()
}

impl ShellState {
    pub(crate) fn new(
        search: crate::picker::App,
        panel_side: PanelSide,
        brain_root: &Path,
    ) -> Self {
        Self {
            main_view: MainView::Tasks,
            focus: Panel::Tasks,
            panel_side,
            brain_rect: None,
            search,
            brain_dir_view: BrainDirView::Search,
            tree: crate::tree::TreeView::empty(brain_root),
            logs_view: None,
            active_brain_tab: BrainTab::Main,
            quit_requested: false,
        }
    }

    pub(crate) const fn request_quit(&mut self) {
        self.quit_requested = true;
    }

    /// Whether a command asked to leave, clearing the request as it answers.
    pub(crate) const fn take_quit_request(&mut self) -> bool {
        let requested = self.quit_requested;
        self.quit_requested = false;
        requested
    }

    pub(crate) const fn main_view(&self) -> MainView {
        self.main_view
    }

    pub(crate) const fn focus(&self) -> Panel {
        self.focus
    }

    pub(crate) const fn focus_tasks(&mut self) {
        self.focus = Panel::Tasks;
    }

    pub(crate) const fn apply_startup_destination(&mut self, destination: StartupDestination) {
        match destination {
            StartupDestination::Tasks => {
                self.main_view = MainView::Tasks;
                self.focus = Panel::Tasks;
            }
            StartupDestination::BrainDirectory => {
                self.main_view = MainView::BrainSearch;
                self.focus = Panel::Tasks;
            }
            StartupDestination::BrainLlm => {
                self.main_view = MainView::Tasks;
                self.focus = Panel::Brain;
            }
            StartupDestination::BrainLlmEmpty => {
                self.main_view = MainView::BrainSearch;
                self.focus = Panel::Brain;
            }
        }
    }

    pub(crate) const fn panel_side(&self) -> PanelSide {
        self.panel_side
    }

    pub(crate) const fn brain_rect(&self) -> Option<Rect> {
        self.brain_rect
    }

    #[cfg(test)]
    pub(crate) fn search_query(&self) -> &str {
        &self.search.query
    }

    pub(crate) const fn logs_view(&self) -> Option<&LogsView> {
        self.logs_view.as_ref()
    }

    pub(crate) fn replace_search_entries(&mut self, entries: &[Entry]) {
        self.search.set_entries(entries);
    }

    pub(crate) fn reload_search_entries(&mut self, entries: &[Entry]) {
        self.search.reload_entries(entries);
    }

    pub(crate) fn selected_search_path(&self) -> Option<PathBuf> {
        self.search.selected_path()
    }

    /// The highlighted brain-directory entry as palette context.
    pub(crate) fn selected_entry_context(&self) -> Option<crate::tui::palette::EntryContext> {
        self.search.selected_entry_context()
    }

    pub(crate) fn render_search(&mut self, frame: &mut Frame, area: Rect) {
        crate::picker::draw_into(frame, &mut self.search, area);
    }

    pub(crate) fn handle_search_input(
        &mut self,
        code: KeyCode,
        ctrl: bool,
        alt: bool,
    ) -> SearchEffect {
        match code {
            KeyCode::Esc => SearchEffect::Quit,
            KeyCode::Char('c') if ctrl => SearchEffect::Quit,
            KeyCode::Enter => self
                .search
                .selected_path()
                .map_or(SearchEffect::None, |path| {
                    if alt {
                        SearchEffect::Explore(path)
                    } else if ctrl {
                        SearchEffect::Reveal(path)
                    } else {
                        SearchEffect::Open(path)
                    }
                }),
            KeyCode::Char('p') if ctrl => SearchEffect::OpenPalette,
            KeyCode::Char('g') if ctrl => self
                .search
                .selected_markdown_path()
                .map_or(SearchEffect::None, SearchEffect::ConfirmPdf),
            KeyCode::Char('r') if ctrl => SearchEffect::Refresh,
            KeyCode::Char('d') if ctrl => self
                .search
                .selected_path()
                .map_or(SearchEffect::None, SearchEffect::ConfirmDelete),
            KeyCode::Up | KeyCode::Char('k') if code == KeyCode::Up || ctrl => {
                self.search.move_up();
                SearchEffect::None
            }
            KeyCode::Down | KeyCode::Char('j') if code == KeyCode::Down || ctrl => {
                self.search.move_down();
                SearchEffect::None
            }
            KeyCode::PageUp => {
                self.search.page_up();
                SearchEffect::None
            }
            KeyCode::PageDown => {
                self.search.page_down();
                SearchEffect::None
            }
            KeyCode::Home => {
                self.search.jump_first();
                SearchEffect::None
            }
            KeyCode::End => {
                self.search.jump_last();
                SearchEffect::None
            }
            KeyCode::Backspace => {
                self.search.pop_query();
                SearchEffect::None
            }
            KeyCode::Char('u') if ctrl => {
                self.search.clear_query();
                SearchEffect::None
            }
            KeyCode::Char('w') if ctrl => {
                self.search.delete_word();
                SearchEffect::None
            }
            KeyCode::Char(character) if !ctrl && !alt => {
                self.search.push_query(character);
                SearchEffect::None
            }
            _ => SearchEffect::None,
        }
    }

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
        self.tree = crate::tree::TreeView::explore(self.search.entries(), &brain_root, target);
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
        self.tree = crate::tree::TreeView::explore(entries, &brain_root, target);
        self.brain_dir_view = BrainDirView::Tree;
    }

    pub(crate) const fn show_search(&mut self) {
        self.brain_dir_view = BrainDirView::Search;
    }

    pub(crate) fn reroot_tree(&mut self, entries: &[Entry], root: &Path) {
        self.tree.reroot(entries, root);
    }

    /// Move the tree to the scope the picker's entries now describe, after a
    /// rescope.
    ///
    /// A re-root rather than a rebuild: the rescope rows are palette rows and
    /// the palette opens over the tree too, so the sub-view behind it must
    /// follow the new scope instead of staying at a root the old scope chose.
    pub(crate) fn rescope_tree(&mut self) {
        let root = crate::tree::root::scope_root(self.search.entries(), self.tree.brain_root());
        self.tree.reroot(self.search.entries(), &root);
    }

    /// Rebuild the tree at the root it is already showing, for a refresh in
    /// place.
    pub(crate) fn rebuild_tree(&mut self) {
        let root = self.tree.root().to_path_buf();
        self.tree.rebuild(self.search.entries(), &root);
    }

    #[cfg(test)]
    pub(crate) fn tree_root(&self) -> &Path {
        self.tree.root()
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
    ) -> SearchEffect {
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

    pub(crate) fn cycle_main_view(&mut self, direction: Dir) {
        self.main_view = self.main_view.step(direction);
    }

    pub(crate) const fn show_main_view(&mut self, main_view: MainView) {
        self.main_view = main_view;
    }

    pub(crate) const fn focus_brain(&mut self) {
        self.focus = Panel::Brain;
    }

    pub(crate) fn toggle_panel_side(&mut self) {
        self.panel_side = self.panel_side.flipped();
    }

    pub(crate) const fn record_brain_rect(&mut self, area: Option<Rect>) {
        self.brain_rect = area;
    }

    pub(crate) fn show_logs(&mut self, logs: LogsView) {
        self.logs_view = Some(logs);
        self.main_view = MainView::Logs;
    }

    pub(crate) fn scroll_logs(&mut self, amount: i16) {
        if let Some(logs) = self.logs_view.as_mut() {
            logs.scroll_by(amount);
        }
    }

    pub(crate) fn scroll_logs_to_start(&mut self) {
        if let Some(logs) = self.logs_view.as_mut() {
            logs.scroll = 0;
        }
    }

    pub(crate) fn scroll_logs_to_end(&mut self) {
        if let Some(logs) = self.logs_view.as_mut() {
            logs.scroll = u16::MAX;
        }
    }

    pub(crate) fn active_brain_tab(&self, open: &[SessionTabId]) -> BrainTab {
        resolve_active_tab(self.active_brain_tab, open)
    }

    pub(crate) fn active_brain_tab_index(&self, open: &[SessionTabId]) -> usize {
        tab_order(open)
            .iter()
            .position(|tab| *tab == self.active_brain_tab(open))
            .unwrap_or(0)
    }

    pub(crate) fn select_brain_tab(
        &mut self,
        requested: BrainTab,
        open: &[SessionTabId],
        panel_visible: bool,
    ) -> bool {
        if !panel_visible {
            return false;
        }
        self.active_brain_tab = resolve_active_tab(requested, open);
        self.focus = Panel::Brain;
        self.active_brain_tab == requested
    }

    pub(crate) fn select_brain_tab_slot(
        &mut self,
        slot: usize,
        open: &[SessionTabId],
        panel_visible: bool,
    ) -> bool {
        tab_for_slot(slot, open).is_some_and(|tab| self.select_brain_tab(tab, open, panel_visible))
    }

    pub(crate) fn cycle_brain_tab(
        &mut self,
        open: &[SessionTabId],
        forward: bool,
        panel_visible: bool,
    ) -> bool {
        if !panel_visible {
            return false;
        }
        let tabs = tab_order(open);
        let current = self.active_brain_tab(open);
        let index = tabs.iter().position(|tab| *tab == current).unwrap_or(0);
        let next = if forward {
            (index + 1) % tabs.len()
        } else {
            (index + tabs.len() - 1) % tabs.len()
        };
        self.active_brain_tab = tabs[next];
        self.focus = Panel::Brain;
        true
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crossterm::event::KeyCode;
    use ratatui::layout::Rect;

    use super::{
        BrainDirView, SearchEffect, ShellState, resolve_active_tab, tab_for_slot, tab_order,
    };
    use crate::main_view::{Dir, MainView, StartupDestination};
    use crate::state::PanelSide;
    use crate::tui::logs_view::{LogKind, LogsView};
    use crate::tui::model::{BrainTab, Panel, SessionTabId};

    const SESSION: SessionTabId = SessionTabId(7);
    const SECOND_SESSION: SessionTabId = SessionTabId(8);

    #[test]
    fn construction_owns_main_view_focus_layout_search_and_logs() {
        let mut state = ShellState::new(
            crate::picker::App::new(&[], ""),
            PanelSide::Right,
            Path::new("/brain"),
        );

        assert_eq!(state.main_view(), MainView::Tasks);
        assert_eq!(state.focus(), Panel::Tasks);
        assert_eq!(state.panel_side(), PanelSide::Right);
        assert_eq!(state.brain_rect(), None);
        assert_eq!(state.active_brain_tab(&[]), BrainTab::Main);

        state.cycle_main_view(Dir::Right);
        state.focus_brain();
        state.toggle_panel_side();
        state.record_brain_rect(Some(Rect::new(40, 0, 40, 24)));
        state.handle_search_input(KeyCode::Char('x'), false, false);
        state.show_logs(LogsView {
            kind: LogKind::Brain,
            text: "one\ntwo".to_owned(),
            scroll: 0,
        });
        state.scroll_logs(1);

        assert_eq!(state.main_view(), MainView::Logs);
        assert_eq!(state.focus(), Panel::Brain);
        assert_eq!(state.panel_side(), PanelSide::Left);
        assert_eq!(state.brain_rect(), Some(Rect::new(40, 0, 40, 24)));
        assert_eq!(state.search_query(), "x");
        assert_eq!(state.logs_view().map(|logs| logs.scroll), Some(1));
    }

    #[test]
    fn startup_destination_sets_the_main_view_and_panel_focus_together() {
        let cases = [
            (StartupDestination::Tasks, MainView::Tasks, Panel::Tasks),
            (
                StartupDestination::BrainDirectory,
                MainView::BrainSearch,
                Panel::Tasks,
            ),
            (StartupDestination::BrainLlm, MainView::Tasks, Panel::Brain),
            (
                StartupDestination::BrainLlmEmpty,
                MainView::BrainSearch,
                Panel::Brain,
            ),
        ];

        for (destination, main_view, focus) in cases {
            let mut state = ShellState::new(
                crate::picker::App::new(&[], ""),
                PanelSide::Right,
                Path::new("/brain"),
            );
            state.apply_startup_destination(destination);
            assert_eq!(state.main_view(), main_view, "{destination:?}");
            assert_eq!(state.focus(), focus, "{destination:?}");
        }
    }

    #[test]
    fn active_tab_selection_resolves_only_open_tabs_and_focuses_the_panel() {
        let mut state = ShellState::new(
            crate::picker::App::new(&[], ""),
            PanelSide::Right,
            Path::new("/brain"),
        );

        assert!(!state.select_brain_tab(BrainTab::Session(SESSION), &[SESSION], false));
        assert!(state.select_brain_tab(BrainTab::Session(SESSION), &[SESSION], true));
        assert_eq!(
            state.active_brain_tab(&[SESSION]),
            BrainTab::Session(SESSION)
        );
        assert_eq!(state.focus(), Panel::Brain);

        state.select_brain_tab(BrainTab::Session(SessionTabId(8)), &[SESSION], true);
        assert_eq!(state.active_brain_tab(&[SESSION]), BrainTab::Main);
    }

    #[test]
    fn tab_order_and_slots_stay_inside_shell_navigation() {
        assert_eq!(tab_order(&[]), vec![BrainTab::Main]);
        assert_eq!(
            tab_order(&[SESSION, SECOND_SESSION]),
            vec![
                BrainTab::Main,
                BrainTab::Session(SESSION),
                BrainTab::Session(SECOND_SESSION)
            ]
        );
        assert_eq!(tab_for_slot(0, &[SESSION]), Some(BrainTab::Main));
        assert_eq!(
            tab_for_slot(1, &[SESSION]),
            Some(BrainTab::Session(SESSION))
        );
        assert_eq!(tab_for_slot(2, &[SESSION]), None);
        assert_eq!(
            resolve_active_tab(BrainTab::Session(SECOND_SESSION), &[SESSION]),
            BrainTab::Main
        );
    }

    #[test]
    fn search_input_stays_local_and_returns_only_external_effects() {
        let mut state = ShellState::new(
            crate::picker::App::new(&[], ""),
            PanelSide::Right,
            Path::new("/brain"),
        );

        assert_eq!(
            state.handle_search_input(KeyCode::Char('x'), false, false),
            SearchEffect::None
        );
        assert_eq!(state.search_query(), "x");
        assert_eq!(
            state.handle_search_input(KeyCode::Char('r'), true, false),
            SearchEffect::Refresh
        );
        assert_eq!(
            state.handle_search_input(KeyCode::Esc, false, false),
            SearchEffect::Quit
        );
    }

    fn entries_fixture() -> Vec<crate::entry::Entry> {
        vec![crate::entry::Entry {
            path: PathBuf::from("/brain/projects/plan.md"),
            display: "~/brain/projects/plan.md".to_owned(),
            bucket: crate::entry::Bucket::Projects,
            is_dir: false,
        }]
    }

    fn shell_state_with_entries() -> ShellState {
        ShellState::new(
            crate::picker::App::new(&entries_fixture(), ""),
            PanelSide::Right,
            Path::new("/brain"),
        )
    }

    /// A scope holding one other bucket, the shape a rescope leaves behind.
    fn capture_entries() -> Vec<crate::entry::Entry> {
        vec![crate::entry::Entry {
            path: PathBuf::from("/brain/capture/inbox.md"),
            display: "~/brain/capture/inbox.md".to_owned(),
            bucket: crate::entry::Bucket::Capture,
            is_dir: false,
        }]
    }

    #[test]
    fn the_brain_directory_starts_on_search_and_toggles_to_the_tree() {
        let mut state = shell_state_with_entries();
        assert_eq!(state.brain_dir_view(), BrainDirView::Search);

        state.show_tree(Path::new("/brain/projects/plan.md"));
        assert_eq!(state.brain_dir_view(), BrainDirView::Tree);

        state.show_search();
        assert_eq!(state.brain_dir_view(), BrainDirView::Search);
    }

    #[test]
    fn exploring_carries_the_search_scope_into_the_tree() {
        // The tree is built from the picker's own entries, so a search scoped
        // to one bucket opens a tree rooted at that bucket -- which is what
        // puts a `../` row at the top to widen back out. Rebuilding from a
        // fresh all-buckets walk instead would root at the brain root, and the
        // `../` row would never appear at all.
        let mut state = shell_state_with_entries();

        state.show_tree(Path::new("/brain/projects/plan.md"));

        assert_eq!(state.tree_root(), Path::new("/brain/projects"));
    }

    #[test]
    fn a_target_outside_the_scope_is_opened_from_a_wider_entry_set() {
        // The palette's target picker walks every bucket, so it hands back
        // paths the current scope does not hold. Built from the picker's own
        // entries, such a tree would root at the scoped bucket -- which the
        // target is not under, so nothing would be selected and a tree scoped
        // to an empty bucket would render blank.
        let mut state = ShellState::new(
            crate::picker::App::new(&capture_entries(), ""),
            PanelSide::Right,
            Path::new("/brain"),
        );

        assert!(state.scope_covers(Path::new("/brain/capture/inbox.md")));
        assert!(!state.scope_covers(Path::new("/brain/projects/plan.md")));

        let mut wide = capture_entries();
        wide.extend(entries_fixture());
        state.show_tree_from(&wide, Path::new("/brain/projects/plan.md"));

        assert_eq!(state.brain_dir_view(), BrainDirView::Tree);
        assert_eq!(state.tree_root(), Path::new("/brain"));
        assert_eq!(
            state.selected_tree_path(),
            Some(PathBuf::from("/brain/projects/plan.md")),
            "the chosen path is what the tree opens to and selects"
        );
    }

    #[test]
    fn rescoping_the_search_moves_the_tree_to_the_new_scope() {
        // Rescoping is a palette row and Ctrl+P works from the tree, so a
        // rescope with the tree showing must change what is on screen.
        let mut state = shell_state_with_entries();
        state.show_tree(Path::new("/brain/projects/plan.md"));
        assert_eq!(state.tree_root(), Path::new("/brain/projects"));

        state.replace_search_entries(&capture_entries());
        state.rescope_tree();

        assert_eq!(state.tree_root(), Path::new("/brain/capture"));
    }

    #[test]
    fn the_parent_row_is_not_an_entry_target() {
        // Highlighting `../` must leave the palette's entry commands without a
        // target, so none of them can act on a row that is really navigation.
        let mut state = shell_state_with_entries();

        state.show_tree(Path::new("/brain/projects/plan.md"));
        state.select_tree_path(Path::new("/brain"));

        assert_eq!(state.selected_tree_path(), Some(PathBuf::from("/brain")));
        assert!(state.selected_tree_entry_context().is_none());
    }

    #[test]
    fn the_tree_sub_view_does_not_add_a_main_view() {
        // The tree replaces the search panel in the same slot; Ctrl+L / Ctrl+H
        // must keep cycling exactly three main views.
        assert_eq!(crate::main_view::MainView::CYCLE.len(), 3);
    }

    #[test]
    fn alt_enter_explores_the_highlighted_entry() {
        let mut state = shell_state_with_entries();

        assert_eq!(
            state.handle_search_input(KeyCode::Enter, false, true),
            SearchEffect::Explore(PathBuf::from("/brain/projects/plan.md"))
        );
    }

    #[test]
    fn alt_enter_with_nothing_highlighted_does_nothing() {
        let mut state = ShellState::new(
            crate::picker::App::new(&[], ""),
            PanelSide::Right,
            Path::new("/brain"),
        );

        assert_eq!(
            state.handle_search_input(KeyCode::Enter, false, true),
            SearchEffect::None
        );
    }

    #[test]
    fn plain_and_ctrl_enter_keep_their_meanings() {
        // Alt is the only new modifier: Enter still opens and Ctrl+Enter still
        // reveals, so exploring cannot have stolen an existing binding.
        let mut state = shell_state_with_entries();

        assert_eq!(
            state.handle_search_input(KeyCode::Enter, false, false),
            SearchEffect::Open(PathBuf::from("/brain/projects/plan.md"))
        );
        assert_eq!(
            state.handle_search_input(KeyCode::Enter, true, false),
            SearchEffect::Reveal(PathBuf::from("/brain/projects/plan.md"))
        );
    }
}
