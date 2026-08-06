
#[test]
fn logs_view_palette_only_lists_log_actions_and_return() {
    let state = PaletteState::new_logs_view(false);
    assert_eq!(
        action_order(&state),
        vec![
            PaletteAction::ToggleReceiver,
            PaletteAction::ShowReceiverServerStatus,
            PaletteAction::ShowReceiverServerLogs,
            PaletteAction::ShowSyncStatus,
            PaletteAction::ShowBrainLogs,
            PaletteAction::ReturnToMainView
        ]
    );
}

#[test]
fn open_links_shown_for_habit_with_notes_url() {
    // A habit has no Linear issue but can carry a notes URL; the command
    // is offered (works_on_habits) and gated only on having ≥ 1 link.
    let with_link = PaletteState::new_task_actions(
        "H1".into(),
        "habit".into(),
        true,
        false,
        false,
        LinkKind::SingleNotes,
    );
    assert!(has_open_links(&with_link));

    let no_link = PaletteState::new_task_actions(
        "H1".into(),
        "habit".into(),
        true,
        false,
        false,
        LinkKind::None,
    );
    assert!(!has_open_links(&no_link));
}

fn action_order(state: &PaletteState) -> Vec<PaletteAction> {
    state.visible().iter().map(|c| c.action).collect()
}

// --- PaletteState: daily-triage alert toggle ---

fn daily_triage_label(state: &PaletteState) -> Option<String> {
    state
        .visible()
        .iter()
        .find(|c| matches!(c.action, PaletteAction::ToggleDailyTriageAlert))
        .map(|c| state.label_for(c))
}
