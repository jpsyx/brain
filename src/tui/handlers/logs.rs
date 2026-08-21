//! Key handling for the diagnostic log main view.

use crossterm::event::KeyCode;

use crate::tui::App;

fn logs_quit_action(code: KeyCode, ctrl: bool) -> bool {
    matches!(code, KeyCode::Char('q' | 'Q')) || (ctrl && matches!(code, KeyCode::Char('c' | 'C')))
}

pub(crate) fn handle_logs_key(app: &mut App, code: KeyCode, ctrl: bool) -> bool {
    if logs_quit_action(code, ctrl) {
        app.shell.show_main_view(crate::main_view::MainView::Tasks);
        return false;
    }
    match code {
        KeyCode::Esc => app.shell.show_main_view(crate::main_view::MainView::Tasks),
        KeyCode::Char('j') | KeyCode::Down => app.shell.scroll_logs(3),
        KeyCode::Char('k') | KeyCode::Up => app.shell.scroll_logs(-3),
        KeyCode::PageDown => app.shell.scroll_logs(20),
        KeyCode::PageUp => app.shell.scroll_logs(-20),
        KeyCode::Char('g') => app.shell.scroll_logs_to_start(),
        KeyCode::Char('G') => app.shell.scroll_logs_to_end(),
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
