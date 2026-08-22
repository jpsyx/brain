use std::path::PathBuf;

use crossterm::event::KeyCode;
use ratatui::{Frame, layout::Rect};

use crate::entry::Entry;
use crate::main_view::{Dir, MainView};
use crate::menu::SearchPalette;
use crate::state::PanelSide;
use crate::tui::{BrainTab, LogsView, Panel, SessionTabId};

pub(crate) struct ShellState {
    main_view: MainView,
    focus: Panel,
    panel_side: PanelSide,
    brain_rect: Option<Rect>,
    search: crate::picker::App,
    logs_view: Option<LogsView>,
    active_brain_tab: BrainTab,
}

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
    pub(crate) fn new(search: crate::picker::App, panel_side: PanelSide) -> Self {
        Self {
            main_view: MainView::Tasks,
            focus: Panel::Tasks,
            panel_side,
            brain_rect: None,
            search,
            logs_view: None,
            active_brain_tab: BrainTab::Main,
        }
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

    pub(crate) fn selected_markdown_search_path(&self) -> Option<PathBuf> {
        self.search.selected_markdown_path()
    }

    pub(crate) fn search_palette(
        &self,
        include_message_brain: bool,
        receiver_enabled: bool,
    ) -> SearchPalette {
        self.search
            .search_palette(self.panel_side, include_message_brain, receiver_enabled)
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
                    if ctrl {
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
    use crossterm::event::KeyCode;
    use ratatui::layout::Rect;

    use super::{SearchEffect, ShellState, resolve_active_tab, tab_for_slot, tab_order};
    use crate::main_view::{Dir, MainView};
    use crate::state::PanelSide;
    use crate::tui::{BrainTab, LogKind, LogsView, Panel, SessionTabId};

    const SESSION: SessionTabId = SessionTabId(7);
    const SECOND_SESSION: SessionTabId = SessionTabId(8);

    #[test]
    fn construction_owns_main_view_focus_layout_search_and_logs() {
        let mut state = ShellState::new(crate::picker::App::new(&[], ""), PanelSide::Right);

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
    fn active_tab_selection_resolves_only_open_tabs_and_focuses_the_panel() {
        let mut state = ShellState::new(crate::picker::App::new(&[], ""), PanelSide::Right);

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
        let mut state = ShellState::new(crate::picker::App::new(&[], ""), PanelSide::Right);

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
}
