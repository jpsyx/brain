
// --- ctrl_opens_palette ---

#[test]
fn ctrl_p_opens_the_palette() {
    assert!(ctrl_opens_palette(KeyCode::Char('p')));
    assert!(ctrl_opens_palette(KeyCode::Char('P')));
}

#[test]
fn ctrl_k_no_longer_opens_the_palette() {
    assert!(!ctrl_opens_palette(KeyCode::Char('k')));
    assert!(!ctrl_opens_palette(KeyCode::Char('t')));
}

// --- ctrl_quits ---

#[test]
fn ctrl_q_quits_the_shell() {
    assert!(ctrl_quits(KeyCode::Char('q'), true));
    assert!(ctrl_quits(KeyCode::Char('Q'), true));
}

#[test]
fn bare_q_is_not_the_global_quit_chord() {
    // Bare `q` quits too, but via the normal-mode handler (tasks panel
    // only). The global chord requires Ctrl so it also reaches us from
    // the brain panel, where bare `q` is forwarded to claude.
    assert!(!ctrl_quits(KeyCode::Char('q'), false));
    assert!(!ctrl_quits(KeyCode::Char('Q'), false));
}
