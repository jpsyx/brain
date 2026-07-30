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

pub(crate) fn draw(f: &mut Frame, app: &mut App<'_>) {
    let area = f.area();

    // Top-level split: if the brain panel is open, it takes half the width on
    // its configured side; the active main view fills the rest. Closed → the
    // main view owns the full width.
    let (main_area, brain_area) = if app.brain.is_some() {
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

    // Modals paint over the panels underneath. Help is app-level (either main
    // view); the tasks modals are only ever open in the tasks view; the
    // brain-search view's own palette / confirm overlays trail the chain.
    if let Some(help) = app.help.as_ref() {
        draw_help(f, help, area);
    } else if let Some(state) = app.palette.as_ref() {
        draw_palette(f, state, area);
    } else if let Some(brain_state) = app.brain_input.as_ref() {
        draw_brain_input(f, brain_state, area);
    } else if let Some(confirm) = app.confirm.as_ref() {
        draw_confirm(f, confirm, area);
    } else if let Some(picker) = app.link_picker.as_ref() {
        draw_link_picker(f, picker, area);
    } else if let Some(menu) = app.search.palette.as_ref() {
        crate::menu::draw_modal(f, menu, area);
    } else if let Some(c) = app.search.confirm.as_ref() {
        crate::confirm::draw_modal(f, c, area);
    }
}
