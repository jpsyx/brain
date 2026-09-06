#[test]
fn daily_triage_toggle_reads_enable_when_alert_disabled() {
    // Seeded from `App::skip_daily_triage_check` at open time; when disabled
    // the command offers to re-enable.
    let state = TaskPalette::new(None, false, false, false, LinkKind::None).with_runtime_context(
        false,
        true,
        Vec::new(),
        Vec::new(),
    );
    assert_eq!(
        daily_triage_label(&state).as_deref(),
        Some("Enable daily triage alert")
    );
}

// --- TaskPalette: skill-session rows ---

#[test]
fn tab_switch_commands_are_hidden_without_a_skill_session_tab() {
    // No additional session is open, so the palette cannot switch tabs.
    let state = TaskPalette::new(None, false, false, false, LinkKind::None);
    let actions = action_order(&state);
    assert!(!actions.contains(&TaskAction::Global(GlobalAction::ShowMainBrainSession)));
    assert!(
        !actions
            .iter()
            .any(|action| matches!(action, TaskAction::Global(GlobalAction::ShowSessionTab(_))))
    );
}

#[test]
fn tab_switch_commands_appear_once_a_skill_session_is_open() {
    // User-session rows capture the stable tab ID and title at open time.
    let state = TaskPalette::new(None, false, false, false, LinkKind::None).with_runtime_context(
        false,
        false,
        Vec::new(),
        vec![crate::tui::state::SessionPaletteEntry::new(
            crate::tui::model::SessionTabId(2),
            "Daily triage",
        )],
    );
    let actions = action_order(&state);
    assert!(actions.contains(&TaskAction::Global(GlobalAction::ShowMainBrainSession)));
    assert!(
        actions.contains(&TaskAction::Global(GlobalAction::ShowSessionTab(
            crate::tui::model::SessionTabId(2)
        )))
    );
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
    let state = TaskPalette::new(None, false, false, false, LinkKind::None).with_runtime_context(
        false,
        false,
        vec![
            (
                crate::skill_session::SkillSessionKey::DailyTriage,
                "Run daily triage".to_owned(),
            ),
            (
                crate::skill_session::SkillSessionKey::Custom(0),
                "Run email triage".to_owned(),
            ),
        ],
        Vec::new(),
    );
    let labels: Vec<String> = state
        .numbered_entries()
        .into_iter()
        .map(|(label, _)| label)
        .collect();

    assert!(
        labels
            .iter()
            .any(|label| label.contains("Run daily triage")),
        "{labels:?}"
    );
    assert!(
        labels
            .iter()
            .any(|label| label.contains("Run email triage")),
        "{labels:?}"
    );
    assert!(
        action_order(&state).contains(&TaskAction::Global(GlobalAction::RunSkillSession(
            crate::skill_session::SkillSessionKey::Custom(0)
        )))
    );
}

#[test]
fn a_running_skill_session_offers_no_start_row() {
    // The seeded `runnable_skill_sessions` already excludes running sessions
    // (that decision is `skill_session::runnable`), so a session showing a
    // focus row must show no start row because a user can't launch it twice.
    let state = TaskPalette::new(None, false, false, false, LinkKind::None).with_runtime_context(
        false,
        false,
        vec![(
            crate::skill_session::SkillSessionKey::Custom(0),
            "Run email triage".to_owned(),
        )],
        vec![crate::tui::state::SessionPaletteEntry::new(
            crate::tui::model::SessionTabId(2),
            "Daily triage",
        )],
    );
    let actions = action_order(&state);

    assert!(
        !actions.contains(&TaskAction::Global(GlobalAction::RunSkillSession(
            crate::skill_session::SkillSessionKey::DailyTriage
        )))
    );
    assert!(
        actions.contains(&TaskAction::Global(GlobalAction::RunSkillSession(
            crate::skill_session::SkillSessionKey::Custom(0)
        )))
    );
}

#[test]
fn full_palette_lists_actions_in_canonical_order() {
    // Task with notes selected: start → complete → message-about →
    // message-global → notes → remove → defer group → other globals.
    let state = TaskPalette::new(Some("T1".into()), false, true, false, LinkKind::None);
    assert_eq!(
        action_order(&state),
        vec![
            TaskAction::StartTask,
            TaskAction::MarkTaskComplete,
            TaskAction::MessageBrainAboutTask,
            TaskAction::Global(GlobalAction::MessageBrain),
            TaskAction::Global(GlobalAction::StartManualSession),
            TaskAction::Global(GlobalAction::ToggleReceiver),
            TaskAction::Global(GlobalAction::ShowReceiverServerStatus),
            TaskAction::Global(GlobalAction::ShowReceiverServerLogs),
            TaskAction::ToggleNotes,
            TaskAction::RemoveTask,
            TaskAction::DeferTask(1),
            TaskAction::DeferTask(7),
            TaskAction::DeferTask(14),
            TaskAction::Global(GlobalAction::OpenHabits),
            TaskAction::Global(GlobalAction::SyncBrainNow),
            TaskAction::Global(GlobalAction::ShowSyncStatus),
            TaskAction::Global(GlobalAction::OpenAgenda),
            TaskAction::Global(GlobalAction::ShowBrainLogs),
            TaskAction::Global(GlobalAction::ToggleDailyTriageAlert),
            TaskAction::Global(GlobalAction::ShowTasks),
        ]
    );
}

#[test]
fn start_rows_sit_with_the_brain_rows_whether_or_not_a_session_is_open() {
    // "Message brain" is always in scope, so anchoring the start rows to it keeps
    // them next to the other brain actions instead of moving when a tab opens.
    let state = TaskPalette::new(None, false, false, false, LinkKind::None).with_runtime_context(
        false,
        false,
        vec![(
            crate::skill_session::SkillSessionKey::DailyTriage,
            "Run daily triage".to_owned(),
        )],
        Vec::new(),
    );

    let actions = action_order(&state);
    let message = actions
        .iter()
        .position(|action| *action == TaskAction::Global(GlobalAction::MessageBrain))
        .expect("message brain row");
    assert_eq!(
        actions[message + 1],
        TaskAction::Global(GlobalAction::StartManualSession)
    );
    assert_eq!(
        actions[message + 2],
        TaskAction::Global(GlobalAction::RunSkillSession(
            crate::skill_session::SkillSessionKey::DailyTriage
        ))
    );
}
