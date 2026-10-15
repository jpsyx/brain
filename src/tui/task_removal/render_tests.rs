use super::{RemovalScreen, RemovalState, lines};
use crate::tasks::removal::model::{FileEdit, Options, RemovalPlan};
#[test]
fn native_task_removal_preview_shows_relationship_effects_and_plain_policy_choices() {
    let mut state = RemovalState {
        generation: 1,
        targets: vec!["T1".into()],
        options: Options::default(),
        operation_id: None,
        notice: String::new(),
        abandon_confirm: false,
        scroll: 0,
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
    };
    let text = lines(&state).join("\n");
    assert!(text.contains("Dependencies T2: T1 -> T3"), "{text}");
    assert!(text.contains("References T2: T1|T9 -> T9"), "{text}");
    assert!(text.contains("Keep prerequisites (rewire)"));
    assert!(text.contains("Remove dependency links (unlink)"));
    state.operation_id = Some("saved-operation".into());
    let text = lines(&state).join("\n");
    assert!(!text.contains("[Enter]"), "{text}");
    assert!(!text.contains("[d]"), "{text}");
    assert!(text.contains("[r] resume"), "{text}");
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
        .draw(|frame| super::draw(frame, &state, frame.area()))
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
    for expected in ["T3", "Last", "T2", "Middle", "[d]"] {
        assert!(text.contains(expected), "{text}");
    }
    assert!(!text.contains("[Enter]"), "{text}");
}
