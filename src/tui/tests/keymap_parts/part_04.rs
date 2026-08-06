
#[test]
fn bare_enter_stays_in_search_mode() {
    // Enter without ctrl is search-specific: exit-search-keep-filter.
    assert!(!search_delegates_ctrl_chord(KeyCode::Enter, false));
}

#[test]
fn ctrl_c_keeps_search_specific_handling() {
    // Ctrl+C is handled search-specifically (it exits `/` instead of
    // quitting the shell), so it must not be bounced to normal-mode.
    assert!(!search_delegates_ctrl_chord(KeyCode::Char('c'), true));
}

// --- search_key_abandons_filter ---

#[test]
fn ctrl_c_in_search_abandons_filter_not_quits() {
    // The motivating bug: Ctrl+C while typing in `/` should leave search
    // mode (clearing the filter), exactly like Esc — never quit the shell.
    assert!(search_key_abandons_filter(KeyCode::Char('c'), true));
}

#[test]
fn esc_in_search_abandons_filter() {
    // Esc has always exited `/` and cleared the filter.
    assert!(search_key_abandons_filter(KeyCode::Esc, false));
}
