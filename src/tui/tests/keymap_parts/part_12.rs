
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
