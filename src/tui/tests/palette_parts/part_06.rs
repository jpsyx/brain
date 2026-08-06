
fn receiver_label(state: &PaletteState) -> Option<String> {
    state
        .visible()
        .iter()
        .find(|c| matches!(c.action, PaletteAction::ToggleReceiver))
        .map(|c| state.label_for(c))
}

#[test]
fn receiver_toggle_label_names_the_next_persistent_action() {
    let mut state = PaletteState::new(None, false, false, false, LinkKind::None, false, false);
    assert_eq!(receiver_label(&state).as_deref(), Some("Enable receiver"));

    state.receiver_enabled = true;
    assert_eq!(receiver_label(&state).as_deref(), Some("Disable receiver"));
}

#[test]
fn daily_triage_toggle_is_globally_available() {
    // A long-running TUI needs to flip the alert mid-session, so the toggle is
    // a global command shown regardless of selection.
    let state = PaletteState::new(None, false, false, false, LinkKind::None, false, false);
    assert!(action_order(&state).contains(&PaletteAction::ToggleDailyTriageAlert));
}

#[test]
fn daily_triage_toggle_reads_disable_when_alert_enabled() {
    // Default state: the alert is enabled, so the command offers to disable it.
    let state = PaletteState::new(None, false, false, false, LinkKind::None, false, false);
    assert_eq!(
        daily_triage_label(&state).as_deref(),
        Some("Disable daily triage alert")
    );
}
