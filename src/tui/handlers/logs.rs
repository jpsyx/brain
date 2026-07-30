//! Key handling for the diagnostic log main view.

use crossterm::event::KeyCode;

use crate::tui::App;

fn logs_quit_action(code: KeyCode, ctrl: bool) -> bool {
    matches!(code, KeyCode::Char('q' | 'Q')) || (ctrl && matches!(code, KeyCode::Char('c' | 'C')))
}

pub(crate) fn handle_logs_key(app: &mut App<'_>, code: KeyCode, ctrl: bool) -> bool {
    if logs_quit_action(code, ctrl) {
        app.main_view = crate::main_view::MainView::Tasks;
        return false;
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

#[cfg(test)]
mod tests {
    use super::logs_quit_action;
    use crossterm::event::KeyCode;

    #[test]
    fn q_and_ctrl_c_return_to_the_main_view() {
        assert!(logs_quit_action(KeyCode::Char('q'), false));
        assert!(logs_quit_action(KeyCode::Char('Q'), false));
        assert!(logs_quit_action(KeyCode::Char('c'), true));
        assert!(!logs_quit_action(KeyCode::Char('c'), false));
    }
}
