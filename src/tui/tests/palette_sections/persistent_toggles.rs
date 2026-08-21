fn receiver_label(state: &TaskPalette) -> Option<String> {
    state
        .visible()
        .iter()
        .find(|c| matches!(c.action, TaskAction::Global(GlobalAction::ToggleReceiver)))
        .map(|row| row.label.clone())
}

#[test]
fn receiver_toggle_label_names_the_next_persistent_action() {
    let state = TaskPalette::new(None, false, false, false, LinkKind::None, false, false);
    assert_eq!(receiver_label(&state).as_deref(), Some("Enable receiver"));

    let state = state.with_runtime_context(true, false, Vec::new(), Vec::new());
    assert_eq!(receiver_label(&state).as_deref(), Some("Disable receiver"));
}

#[test]
fn daily_triage_toggle_is_globally_available() {
    // A long-running TUI needs to flip the alert mid-session, so the toggle is
    // a global command shown regardless of selection.
    let state = TaskPalette::new(None, false, false, false, LinkKind::None, false, false);
    assert!(
        action_order(&state).contains(&TaskAction::Global(GlobalAction::ToggleDailyTriageAlert))
    );
}

#[test]
fn daily_triage_toggle_reads_disable_when_alert_enabled() {
    // Default state: the alert is enabled, so the command offers to disable it.
    let state = TaskPalette::new(None, false, false, false, LinkKind::None, false, false);
    assert_eq!(
        daily_triage_label(&state).as_deref(),
        Some("Disable daily triage alert")
    );
}
