use super::{RemovalScreen, RemovalState, lines};
use crate::tasks::removal::model::{FileEdit, Options, RemovalPlan};
pub(in crate::tui::task_removal) fn preview_state() -> RemovalState {
    RemovalState {
        generation: 1,
        targets: vec!["T1".into()],
        options: Options::default(),
        operation_id: None,
        notice: String::new(),
        abandon_confirm: false,
        scroll: 0,
        selected: 0,
        screen: RemovalScreen::Preview(RemovalPlan {
            presentation_labels: std::collections::BTreeMap::new(),
            maintenance: None,
            fingerprint: "consent".into(),
            targets: vec![],
            options: Options::default(),
            edits: vec![FileEdit {
                relative: "tasks/tasks.csv".into(),
                before: Some(b"task_id,blocked_by,see_also\nT2,T1,T1|T9\n".to_vec()),
                after: b"task_id,blocked_by,see_also\nT2,T3,T9\n".to_vec(),
            }],
            external: vec![],
            successors: std::collections::BTreeMap::new(),
            blockers: vec![],
            warnings: vec![],
        }),
    }
}

#[test]
fn native_task_removal_preview_shows_relationship_effects_and_plain_policy_choices() {
    let mut state = preview_state();
    let text = lines(&state).join("\n");
    assert!(
        text.contains("T2 will wait for T3 (previously T1)."),
        "{text}"
    );
    assert!(
        text.contains("T2 will link to T9 (previously T1, T9)."),
        "{text}"
    );
    assert!(!text.contains("Consent:"), "{text}");
    assert!(!text.contains("tasks/tasks.csv"), "{text}");
    state.operation_id = Some("saved-operation".into());
    let text = lines(&state).join("\n");
    assert!(!text.contains("[Enter]"), "{text}");
    assert!(!text.contains("[d]"), "{text}");
    assert!(
        super::choices(&state)
            .iter()
            .any(|(_, label)| label == "Retry deletion")
    );
    state.screen = RemovalScreen::Failed(crate::tasks::removal::model::Problem::new(
        "failed",
        "failed",
        serde_json::json!({}),
    ));
    assert!(!lines(&state).join("\n").contains("[p]"));
    state.screen = RemovalScreen::Running {
        operation_id: state.operation_id.clone(),
    };
    assert!(!lines(&state).join("\n").contains("[r]"));
    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| super::draw(frame, &mut state, frame.area()))
        .unwrap();
    assert!(
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .any(|cell| cell.symbol() == "╭"
                && cell.fg == crate::tui::modal_state::ConfirmIntent::Danger.accent())
    );
}

#[test]
fn blocked_modal_identifies_the_named_dependent_edge_without_confirm() {
    let state = RemovalState {
        generation: 1,
        targets: vec!["T2".into()],
        options: Options::default(),
        operation_id: None,
        notice: String::new(),
        abandon_confirm: false,
        scroll: 0,
        selected: 0,
        screen: RemovalScreen::Preview(RemovalPlan {
            presentation_labels: std::collections::BTreeMap::new(),
            maintenance: None,
            fingerprint: "consent".into(),
            targets: vec![],
            options: Options::default(),
            edits: vec![],
            external: vec![],
            successors: std::collections::BTreeMap::new(),
            warnings: vec![],
            blockers: vec![crate::tasks::removal::model::Problem::new(
                "dependents_exist",
                "T3 (Last) depends on removed T2 (Middle)",
                serde_json::json!({"task_id":"T3", "task_name":"Last", "prerequisite":"T2", "prerequisite_name":"Middle"}),
            )],
        }),
    };
    let text = lines(&state).join("\n");
    for expected in ["T3", "Last", "T2", "Middle", "waiting"] {
        assert!(text.contains(expected), "{text}");
    }
    assert!(!text.contains("[Enter]"), "{text}");
}

#[test]
fn native_task_removal_narrow_window_keeps_selection_and_scroll_help_visible() {
    let mut state = preview_state();
    state.notice = "Long detail ".repeat(40);
    state.scroll = u16::MAX;
    state.selected = 3;
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(44, 16)).unwrap();
    terminal
        .draw(|frame| super::draw(frame, &mut state, frame.area()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let text: String = buffer
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect();
    assert!(text.contains("PgUp/PgDn"), "{text}");
    assert!(
        state.scroll < u16::MAX,
        "scroll must clamp at the end so PageUp responds immediately"
    );
    assert!(text.contains("Esc close"), "{text}");
    assert!(
        buffer
            .content
            .iter()
            .any(|c| c.bg == crate::render::ACCENT_RED)
    );
}
