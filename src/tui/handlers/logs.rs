//! Key handling for the diagnostic log main view.

use crossterm::event::KeyCode;

use crate::tui::App;

pub(crate) fn handle_logs_key(app: &mut App<'_>, code: KeyCode, ctrl: bool) -> bool {
    if ctrl && matches!(code, KeyCode::Char('c' | 'q')) {
        return true;
    }
    match code {
        KeyCode::Esc => app.main_view = crate::main_view::MainView::Tasks,
        KeyCode::Char('j') | KeyCode::Down => {
            if let Some(logs) = app.logs_view.as_mut() {
                logs.scroll_by(3);
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if let Some(logs) = app.logs_view.as_mut() {
                logs.scroll_by(-3);
            }
        }
        KeyCode::PageDown => {
            if let Some(logs) = app.logs_view.as_mut() {
                logs.scroll_by(20);
            }
        }
        KeyCode::PageUp => {
            if let Some(logs) = app.logs_view.as_mut() {
                logs.scroll_by(-20);
            }
        }
        KeyCode::Char('g') => {
            if let Some(logs) = app.logs_view.as_mut() {
                logs.scroll = 0;
            }
        }
        KeyCode::Char('G') => {
            if let Some(logs) = app.logs_view.as_mut() {
                logs.scroll = u16::MAX;
            }
        }
        _ => {}
    }
    false
}
