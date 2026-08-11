
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

// --- PaletteState: skill-session rows ---

#[test]
fn tab_switch_commands_are_hidden_without_a_skill_session_tab() {
    // No skill session open → the palette must not offer to switch tabs. This
    // is the `is_visible: if_skill_session_open` gate.
    let state = PaletteState::new(None, false, false, false, LinkKind::None, false, false);
    let actions = action_order(&state);
    assert!(!actions.contains(&PaletteAction::ShowMainBrainSession));
    assert!(
        !actions
            .iter()
            .any(|action| matches!(action, PaletteAction::ShowSkillSession(_)))
    );
}

#[test]
fn tab_switch_commands_appear_once_a_skill_session_is_open() {
    // `open_skill_sessions` is seeded from the running tabs at open time. With
    // one set, the main-session row and that session's focus row show. The
    // palette remains the reliable alternative to the terminal-flaky Alt+digit.
    let mut state = PaletteState::new(None, false, false, false, LinkKind::None, false, false);
    state.open_skill_sessions = vec![(crate::skill_session::SkillSessionKey::DailyTriage, "Daily triage".to_owned())];
    let actions = action_order(&state);
    assert!(actions.contains(&PaletteAction::ShowMainBrainSession));
    assert!(actions.contains(&PaletteAction::ShowSkillSession(
        crate::skill_session::SkillSessionKey::DailyTriage
    )));
    assert!(
        state
            .numbered_entries()
            .iter()
            .any(|(label, _)| label.contains("Show Daily triage session")),
        "{:?}",
        state.numbered_entries()
    );
}

#[test]
fn each_offered_skill_session_gets_its_configured_palette_label() {
    let mut state = PaletteState::new(None, false, false, false, LinkKind::None, false, false);
    state.runnable_skill_sessions = vec![
        (crate::skill_session::SkillSessionKey::DailyTriage, "Run daily triage".to_owned()),
        (crate::skill_session::SkillSessionKey::Custom(0), "Run email triage".to_owned()),
    ];
    let labels: Vec<String> = state
        .numbered_entries()
        .into_iter()
        .map(|(label, _)| label)
        .collect();

    assert!(
        labels.iter().any(|label| label.contains("Run daily triage")),
        "{labels:?}"
    );
    assert!(
        labels.iter().any(|label| label.contains("Run email triage")),
        "{labels:?}"
    );
    assert!(action_order(&state).contains(&PaletteAction::RunSkillSession(
        crate::skill_session::SkillSessionKey::Custom(0)
    )));
}

#[test]
fn a_running_skill_session_offers_no_start_row() {
    // The seeded `runnable_skill_sessions` already excludes running sessions
    // (that decision is `skill_session::runnable`), so a session showing a
    // focus row must show no start row — a user can't launch it twice.
    let mut state = PaletteState::new(None, false, false, false, LinkKind::None, false, false);
    state.runnable_skill_sessions = vec![(crate::skill_session::SkillSessionKey::Custom(0), "Run email triage".to_owned())];
    state.open_skill_sessions = vec![(crate::skill_session::SkillSessionKey::DailyTriage, "Daily triage".to_owned())];
    let actions = action_order(&state);

    assert!(!actions.contains(&PaletteAction::RunSkillSession(
        crate::skill_session::SkillSessionKey::DailyTriage
    )));
    assert!(actions.contains(&PaletteAction::RunSkillSession(
        crate::skill_session::SkillSessionKey::Custom(0)
    )));
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

#[test]
fn start_rows_sit_with_the_brain_rows_whether_or_not_a_session_is_open() {
    // "Message brain" is always in scope, so anchoring the start rows to it keeps
    // them next to the other brain actions instead of moving when a tab opens.
    let mut state = PaletteState::new(None, false, false, false, LinkKind::None, false, false);
    state.runnable_skill_sessions = vec![(
        crate::skill_session::SkillSessionKey::DailyTriage,
        "Run daily triage".to_owned(),
    )];

    let actions = action_order(&state);
    let message = actions
        .iter()
        .position(|action| *action == PaletteAction::SendBrainMessage)
        .expect("message brain row");
    assert_eq!(
        actions[message + 1],
        PaletteAction::RunSkillSession(crate::skill_session::SkillSessionKey::DailyTriage)
    );
}
