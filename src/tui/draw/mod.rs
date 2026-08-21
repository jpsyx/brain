//! The main draw entry plus the tasks / brain panels and small draw helpers:
//!   - `tasks_panel` — the tasks main-view panel
//!   - `brain_panel` — the `claude` PTY panel
//!   - `layout`      — wrapped-row offsets, selection band, brighten, flash,
//!     modal centering

mod brain_panel;
mod layout;
mod tasks_panel;

pub(crate) use brain_panel::*;
pub(crate) use layout::*;
pub(crate) use tasks_panel::*;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
};

use crate::main_view::MainView;
use crate::state::PanelSide;
use crate::tui::*;

pub(crate) fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();

    // Top-level split: if the brain panel is open, it takes half the width on
    // its configured side; the active main view fills the rest. Closed → the
    // main view owns the full width.
    let (main_area, brain_area) = if app.any_brain_panel_visible() {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);
        match app.panel_side {
            PanelSide::Right => (cols[0], Some(cols[1])),
            PanelSide::Left => (cols[1], Some(cols[0])),
        }
    } else {
        (area, None)
    };

    // Record the brain panel's rect so the mouse handler can hit-test the
    // wheel against it (None when the main view owns the full width).
    app.brain_rect = brain_area;

    match app.main_view {
        MainView::Tasks => draw_tasks(f, app, main_area),
        MainView::BrainSearch => crate::picker::draw_into(f, &mut app.search, main_area),
        MainView::Logs => draw_logs(f, app, main_area),
    }
    if let Some(brain_rect) = brain_area {
        draw_brain(f, app, brain_rect);
    }

    // The same enum that routes input selects the one modal drawn over both
    // panels. No precedence chain exists because simultaneous overlays cannot
    // be represented.
    match app.overlay.as_ref() {
        Some(Overlay::TaskPalette(state)) => draw_palette(f, state, area),
        Some(Overlay::BrainInput(state)) => draw_brain_input(f, state, area),
        Some(Overlay::TaskConfirmation(state)) => draw_confirm(f, state, area),
        Some(Overlay::SearchPalette(state)) => crate::menu::draw_modal(f, state, area),
        Some(Overlay::SearchConfirmation(state)) => crate::confirm::draw_modal(f, state, area),
        Some(Overlay::LinkPicker(state)) => draw_link_picker(f, state, area),
        Some(Overlay::AssigneeFilter(state)) => draw_assignee_filter(f, state, area),
        Some(Overlay::Help(state)) => draw_help(f, state, area),
        Some(Overlay::SyncLog(state)) => {
            // Re-read every frame so the modal tails a running sync.
            let live = crate::sync::current::live_log(app.command_context.workspace.paths());
            draw_sync_log(f, state, live.as_deref(), area);
        }
        None => {}
    }
}
