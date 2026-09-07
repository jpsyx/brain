#[test]
fn both_palettes_share_stable_session_actions_and_no_direct_shortcuts() {
    use crate::menu::{SearchAction, Targets, items};
    use crate::state::PanelSide;
    use crate::tui::model::SessionTabId;
    use crate::tui::state::SessionPaletteEntry;

    let sessions = vec![
        SessionPaletteEntry::new(SessionTabId(7), "Atlas"),
        SessionPaletteEntry::new(SessionTabId(12), "Daily triage"),
    ];
    let tasks = TaskPalette::new(None, false, false, false, LinkKind::None).with_runtime_context(
        false,
        false,
        Vec::new(),
        sessions.clone(),
    );
    let search = items(
        PanelSide::Right,
        true,
        &Targets {
            user_sessions: sessions,
            ..Targets::default()
        },
    );
    let expected = [
        ("Start new brain session", GlobalAction::StartManualSession),
        ("Rename session", GlobalAction::RenameSession),
        (
            "Show main brain session",
            GlobalAction::ShowMainBrainSession,
        ),
        (
            "Show Atlas session",
            GlobalAction::ShowSessionTab(SessionTabId(7)),
        ),
        (
            "Close Atlas session",
            GlobalAction::CloseSessionTab(SessionTabId(7)),
        ),
        (
            "Show Daily triage session",
            GlobalAction::ShowSessionTab(SessionTabId(12)),
        ),
        (
            "Close Daily triage session",
            GlobalAction::CloseSessionTab(SessionTabId(12)),
        ),
    ];
    for (label, action) in expected {
        let task_row = tasks
            .rows()
            .iter()
            .find(|row| row.action == TaskAction::Global(action))
            .unwrap();
        let search_row = search
            .iter()
            .find(|row| row.action == SearchAction::Global(action))
            .unwrap();
        assert_eq!(task_row.label, label);
        assert_eq!(search_row.label, label);
        assert_eq!(task_row.shortcut, None);
        assert_eq!(search_row.shortcut, None);
    }
    let task_labels: Vec<_> = tasks.rows().iter().map(|row| row.label.as_str()).collect();
    let search_labels: Vec<_> = search.iter().map(|row| row.label.as_str()).collect();
    for labels in [task_labels, search_labels] {
        for forbidden in [
            "Close brain",
            "Close Brain session",
            "Show Receiver · SMS session",
            "Close Receiver · SMS session",
        ] {
            assert!(!labels.contains(&forbidden));
        }
        let message = labels
            .iter()
            .position(|label| *label == "Message brain")
            .unwrap();
        assert_eq!(
            &labels[message + 1..message + 8],
            &expected.map(|(label, _)| label)
        );
    }
}

#[test]
fn both_global_palettes_offer_session_rename_without_a_direct_shortcut() {
    use crate::menu::{Targets, items};
    use crate::state::PanelSide;

    let tasks = TaskPalette::new(None, false, false, false, LinkKind::None);
    let search = items(PanelSide::Right, true, &Targets::default());

    let task_row = tasks
        .rows()
        .iter()
        .find(|row| row.label == "Rename session")
        .expect("task palette rename row");
    let search_row = search
        .iter()
        .find(|row| row.label == "Rename session")
        .expect("search palette rename row");
    assert_eq!(task_row.shortcut, None);
    assert_eq!(search_row.shortcut, None);
}

#[test]
fn logs_and_task_actions_keep_session_actions_out_of_scope() {
    use crate::tui::model::SessionTabId;
    use crate::tui::state::SessionPaletteEntry;
    for palette in [
        TaskPalette::new_logs_view(false),
        TaskPalette::new_task_actions(
            "T1".into(),
            "Task".into(),
            false,
            true,
            false,
            LinkKind::None,
        ),
    ] {
        let palette = palette.with_runtime_context(
            false,
            false,
            vec![(
                crate::skill_session::SkillSessionKey::DailyTriage,
                "Run daily triage".into(),
            )],
            vec![SessionPaletteEntry::new(SessionTabId(4), "Atlas")],
        );
        assert!(!palette.rows().iter().any(|row| matches!(
            row.action,
            TaskAction::Global(
                GlobalAction::MessageBrain
                    | GlobalAction::StartManualSession
                    | GlobalAction::RenameSession
                    | GlobalAction::ShowMainBrainSession
                    | GlobalAction::ShowSessionTab(_)
                    | GlobalAction::CloseSessionTab(_)
                    | GlobalAction::RunSkillSession(_)
            )
        )));
    }
}
