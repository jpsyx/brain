//! The main draw entry plus the tasks / brain panels and small draw helpers:
//!   - `tasks_panel` — the tasks main-view panel
//!   - `brain_panel` — the `claude` PTY panel
//!   - `layout`      — wrapped-row offsets, selection band, brighten, flash,
//!     modal centering

mod brain_panel;
pub(crate) mod layout;
pub(super) mod tasks_panel;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
};

use crate::main_view::MainView;
use crate::state::PanelSide;
use crate::tui::App;
use crate::tui::draw::brain_panel::{BrainPanelContext, draw_brain};
use crate::tui::draw::tasks_panel::{TasksPanelContext, draw_tasks};
use crate::tui::draw_assignee::draw_assignee_filter;
use crate::tui::draw_help::draw_help;
use crate::tui::draw_modals::{draw_brain_input, draw_confirm, draw_link_picker};
use crate::tui::draw_palette::draw_palette;
use crate::tui::draw_sync_log::draw_sync_log;
use crate::tui::logs_view::draw_logs;
use crate::tui::model::Panel;
use crate::tui::overlay::Overlay;

pub(crate) fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();

    // Top-level split: if the brain panel is open, it takes half the width on
    // its configured side; the active main view fills the rest. Closed → the
    // main view owns the full width.
    let (main_area, brain_area) = if app.brain.any_panel_visible() {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);
        match app.shell.panel_side() {
            PanelSide::Right => (cols[0], Some(cols[1])),
            PanelSide::Left => (cols[1], Some(cols[0])),
        }
    } else {
        (area, None)
    };

    // Record the brain panel's rect so the mouse handler can hit-test the
    // wheel against it (None when the main view owns the full width).
    app.shell.record_brain_rect(brain_area);

    match app.shell.main_view() {
        MainView::Tasks => {
            let context = TasksPanelContext {
                split_pane_open: app.brain.any_panel_visible(),
                focused: app.shell.focus() == Panel::Tasks,
                flash: app.status.flash(),
                sync_status: app.status.sync_status(),
                persistent_warning: app.status.persistent_warning(),
            };
            draw_tasks(f, &mut app.tasks, &context, main_area);
        }
        MainView::BrainSearch => {
            app.shell.render_search(f, main_area);
        }
        MainView::Logs => draw_logs(f, app.shell.logs_view(), main_area),
    }
    if let Some(brain_rect) = brain_area {
        let workspace_name = app.context.workspace().name().as_str().to_owned();
        let agent = app.context.agent_kind().label().to_owned();
        let alert = app.status.alert().map(str::to_owned);
        let mut context = BrainPanelContext {
            focused: app.shell.focus() == Panel::Brain,
            tab_titles: app.brain.tab_titles(),
            active_tab: app.effective_brain_tab(),
            active_index: app.active_brain_tab_index(),
            workspace_name,
            session_title: app.active_brain_tab_title().map(str::to_owned),
            agent,
            alert,
            controller: app.active_brain_controller_mut(),
        };
        draw_brain(f, &mut context, brain_rect);
    }

    // The same enum that routes input selects the one modal drawn over both
    // panels. No precedence chain exists because simultaneous overlays cannot
    // be represented.
    match app.overlay.as_ref() {
        Some(Overlay::TaskPalette(state)) => draw_palette(f, state, area),
        Some(Overlay::BrainInput(state)) => draw_brain_input(f, state, area),
        Some(Overlay::TaskConfirmation(state)) => draw_confirm(f, state, area),
        Some(Overlay::SearchPalette(state)) => crate::menu::draw_modal(f, state, main_area),
        Some(Overlay::SearchConfirmation(state)) => {
            crate::confirm::draw_modal(f, state, main_area);
        }
        Some(Overlay::LinkPicker(state)) => draw_link_picker(f, state, area),
        Some(Overlay::AssigneeFilter(state)) => draw_assignee_filter(f, state, area),
        Some(Overlay::Help(state)) => draw_help(f, state, area),
        Some(Overlay::SyncLog(state)) => {
            // Re-read every frame so the modal tails a running sync.
            let live = crate::sync::current::live_log(app.context.workspace().paths());
            draw_sync_log(f, state, live.as_deref(), area);
        }
        None => {}
    }
}
