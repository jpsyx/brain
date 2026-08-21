use ratatui::layout::Rect;

use crate::main_view::{Dir, MainView};
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

pub(crate) fn resolve_active_tab(requested: BrainTab, open: &[SessionTabId]) -> BrainTab {
    match requested {
        BrainTab::Session(id) if open.contains(&id) => BrainTab::Session(id),
        _ => BrainTab::Main,
    }
}

pub(crate) fn tab_order(open: &[SessionTabId]) -> Vec<BrainTab> {
    let mut tabs = vec![BrainTab::Main];
    tabs.extend(open.iter().copied().map(BrainTab::Session));
    tabs
}

pub(crate) fn tab_for_slot(slot: usize, open: &[SessionTabId]) -> Option<BrainTab> {
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

    pub(crate) const fn search(&self) -> &crate::picker::App {
        &self.search
    }

    pub(crate) const fn search_mut(&mut self) -> &mut crate::picker::App {
        &mut self.search
    }

    #[cfg(test)]
    pub(crate) fn search_query(&self) -> &str {
        &self.search.query
    }

    pub(crate) const fn logs_view(&self) -> Option<&LogsView> {
        self.logs_view.as_ref()
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
    use ratatui::layout::Rect;

    use super::ShellState;
    use crate::main_view::{Dir, MainView};
    use crate::state::PanelSide;
    use crate::tui::{BrainTab, LogKind, LogsView, Panel, SessionTabId};

    const SESSION: SessionTabId = SessionTabId(7);

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
        state.search_mut().push_query('x');
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
}
