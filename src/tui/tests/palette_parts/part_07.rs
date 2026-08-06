
#[test]
fn daily_triage_toggle_reads_enable_when_alert_disabled() {
    // Seeded from `App::skip_daily_triage_check` at open time; when disabled
    // the command offers to re-enable.
    let mut state = PaletteState::new(None, false, false, false, LinkKind::None, false, false);
    state.daily_triage_alert_disabled = true;
    assert_eq!(
        daily_triage_label(&state).as_deref(),
        Some("Enable daily triage alert")
    );
}

// --- PaletteState: brain-tab switch commands (triage tab) ---

#[test]
fn triage_switch_commands_are_hidden_without_a_triage_tab() {
    // No triage tab open → the palette must not offer to switch to it. This is
    // the `is_visible: if_triage_open` gate.
    let state = PaletteState::new(None, false, false, false, LinkKind::None, false, false);
    let actions = action_order(&state);
    assert!(!actions.contains(&PaletteAction::ShowMainBrainSession));
    assert!(!actions.contains(&PaletteAction::ShowDailyTriageSession));
}

#[test]
fn triage_switch_commands_appear_while_a_triage_tab_is_open() {
    // `triage_open` is seeded from `App::triage_brain.is_some()` at open time,
    // With it set, both tab-switch rows show. The palette remains the reliable
    // alternative to the terminal-flaky Alt+1 / Alt+2.
    let mut state = PaletteState::new(None, false, false, false, LinkKind::None, false, false);
    state.triage_open = true;
    let actions = action_order(&state);
    assert!(actions.contains(&PaletteAction::ShowMainBrainSession));
    assert!(actions.contains(&PaletteAction::ShowDailyTriageSession));
}

#[test]
fn full_palette_lists_actions_in_canonical_order() {
    // Task with notes selected: start → complete → message-about →
    // message-global → notes → remove → defer group → other globals.
    let state = PaletteState::new(
        Some("T1".into()),
        false,
        true,
        false,
        LinkKind::None,
        false,
        false,
    );
    assert_eq!(
        action_order(&state),
        vec![
            PaletteAction::StartTask,
            PaletteAction::MarkTaskComplete,
            PaletteAction::MessageBrainAboutTask,
            PaletteAction::SendBrainMessage,
            PaletteAction::ToggleReceiver,
            PaletteAction::ShowReceiverServerStatus,
            PaletteAction::ShowReceiverServerLogs,
            PaletteAction::ToggleNotes,
            PaletteAction::RemoveTask,
            PaletteAction::DeferTask(1),
            PaletteAction::DeferTask(7),
            PaletteAction::DeferTask(14),
            PaletteAction::OpenHabitsInBrowser,
            PaletteAction::SyncBrainNow,
            PaletteAction::ShowSyncStatus,
            PaletteAction::OpenAgenda,
            PaletteAction::ShowBrainLogs,
            PaletteAction::ToggleDailyTriageAlert,
            PaletteAction::ReturnToMainView,
        ]
    );
}
