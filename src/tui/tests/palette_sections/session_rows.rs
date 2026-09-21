#[test]
fn the_static_session_commands_are_always_offered() {
    // Close and show-main used to appear only once a second tab existed; they
    // are declared commands now, and their pickers handle the empty case.
    let listed = actions(&palette(&shared_workspace()));

    for action in [
        GlobalAction::MessageBrain,
        GlobalAction::StartManualSession,
        GlobalAction::RenameSession,
        GlobalAction::CloseSession,
        GlobalAction::NewConversation,
        GlobalAction::ShowMainBrainSession,
    ] {
        assert!(listed.contains(&Command::Global(action)), "{action:?}");
    }
}

#[test]
fn open_tabs_add_a_stable_show_row_each_after_the_session_block() {
    let state = palette(&PaletteContext {
        user_sessions: vec![
            SessionPaletteEntry::new(SessionTabId(7), "Atlas"),
            SessionPaletteEntry::new(SessionTabId(12), "Daily triage"),
        ],
        ..shared_workspace()
    });
    let listed = actions(&state);

    let anchor = listed
        .iter()
        .position(|command| *command == Command::Global(GlobalAction::ShowMainBrainSession))
        .expect("the show-main row anchors the per-session rows");
    assert_eq!(
        &listed[anchor + 1..=anchor + 2],
        &[
            Command::Global(GlobalAction::ShowSessionTab(SessionTabId(7))),
            Command::Global(GlobalAction::ShowSessionTab(SessionTabId(12))),
        ]
    );
    assert_eq!(
        label_of(
            &state,
            Command::Global(GlobalAction::ShowSessionTab(SessionTabId(7)))
        )
        .as_deref(),
        Some("Show Atlas session")
    );
}

#[test]
fn each_runnable_skill_session_gets_its_configured_label() {
    let state = palette(&PaletteContext {
        runnable_skill_sessions: vec![
            (SkillSessionKey::DailyTriage, "Run daily triage".to_owned()),
            (SkillSessionKey::Custom(0), "Run email triage".to_owned()),
        ],
        ..shared_workspace()
    });

    assert_eq!(
        label_of(
            &state,
            Command::Global(GlobalAction::RunSkillSession(SkillSessionKey::Custom(0)))
        )
        .as_deref(),
        Some("Run email triage")
    );
    assert!(
        actions(&state).contains(&Command::Global(GlobalAction::RunSkillSession(
            SkillSessionKey::DailyTriage
        )))
    );
}

#[test]
fn a_running_skill_session_offers_no_start_row() {
    // The seeded `runnable_skill_sessions` already excludes running sessions,
    // so a session showing a focus row shows no start row.
    let state = palette(&PaletteContext {
        runnable_skill_sessions: vec![(SkillSessionKey::Custom(0), "Run email triage".to_owned())],
        user_sessions: vec![SessionPaletteEntry::new(SessionTabId(2), "Daily triage")],
        ..shared_workspace()
    });
    let listed = actions(&state);

    assert!(!listed.contains(&Command::Global(GlobalAction::RunSkillSession(
        SkillSessionKey::DailyTriage
    ))));
    assert!(listed.contains(&Command::Global(GlobalAction::RunSkillSession(
        SkillSessionKey::Custom(0)
    ))));
}

#[test]
fn no_session_row_carries_a_direct_shortcut_annotation() {
    let state = palette(&PaletteContext {
        user_sessions: vec![SessionPaletteEntry::new(SessionTabId(7), "Atlas")],
        runnable_skill_sessions: vec![(
            SkillSessionKey::DailyTriage,
            "Run daily triage".to_owned(),
        )],
        ..shared_workspace()
    });

    for row in state.visible() {
        if matches!(
            row.action,
            Command::Global(GlobalAction::ShowSessionTab(_) | GlobalAction::RunSkillSession(_))
        ) {
            assert_eq!(row.shortcut, None, "{}", row.label);
        }
    }
}
