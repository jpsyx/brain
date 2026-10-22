#[test]
fn ctrl_l_no_longer_opens_links() {
    // The binding moved off Ctrl+L; bare `l` (notes toggle) and Ctrl+L
    // must not trigger the open action.
    assert!(!ctrl_opens_links(KeyCode::Char('l'), true));
    assert!(!ctrl_opens_links(KeyCode::Char('k'), true));
    assert!(!ctrl_opens_links(KeyCode::Char('d'), true));
}

// --- ctrl_removes_task ---

#[test]
fn ctrl_backspace_removes_task() {
    assert!(ctrl_removes_task(KeyCode::Backspace, true));
}

#[test]
fn bare_backspace_does_not_remove_task() {
    // Plain Backspace is a no-op in the task list: it's too easy to hit
    // by accident, so removal requires the Ctrl modifier.
    assert!(!ctrl_removes_task(KeyCode::Backspace, false));
}

#[test]
fn ctrl_other_keys_do_not_remove_task() {
    assert!(!ctrl_removes_task(KeyCode::Char('d'), true));
    assert!(!ctrl_removes_task(KeyCode::Delete, true));
}

#[test]
fn delete_confirmation_requires_a_plain_d_press() {
    use crate::tui::keymap::confirms_task_removal;
    use crossterm::event::{KeyEvent, KeyEventKind, KeyModifiers};

    assert!(confirms_task_removal(&KeyEvent::new(
        KeyCode::Char('d'),
        KeyModifiers::NONE
    )));
    for modifiers in [
        KeyModifiers::CONTROL,
        KeyModifiers::ALT,
        KeyModifiers::SHIFT,
        KeyModifiers::SUPER,
    ] {
        assert!(!confirms_task_removal(&KeyEvent::new(
            KeyCode::Char('d'),
            modifiers
        )));
    }
    for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
        assert!(!confirms_task_removal(&KeyEvent::new_with_kind(
            KeyCode::Char('d'),
            KeyModifiers::NONE,
            kind
        )));
    }
    for code in [KeyCode::Char('D'), KeyCode::Enter, KeyCode::Char('y')] {
        assert!(!confirms_task_removal(&KeyEvent::new(
            code,
            KeyModifiers::NONE
        )));
    }
}
