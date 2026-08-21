#[test]
fn sync_brain_palette_command_has_no_shortcut() {
    use crate::tui::palette::shortcut_for;

    let state = TaskPalette::new(None, false, false, false, LinkKind::None, false, false);
    let rows = state.numbered_entries();

    assert!(
        rows.iter()
            .any(|(label, shortcut)| label.contains("Sync brain now") && shortcut.is_none()),
        "{rows:?}"
    );
    assert_eq!(
        shortcut_for(TaskAction::Global(GlobalAction::SyncBrainNow)),
        None
    );
}

#[test]
fn task_actions_modal_palette_keeps_order_minus_globals() {
    // Same relative order, with the global commands filtered out.
    let state = TaskPalette::new_task_actions(
        "T1".into(),
        "task".into(),
        false,
        true,
        false,
        LinkKind::None,
    );
    assert_eq!(
        action_order(&state),
        vec![
            TaskAction::StartTask,
            TaskAction::MarkTaskComplete,
            TaskAction::MessageBrainAboutTask,
            TaskAction::ToggleNotes,
            TaskAction::RemoveTask,
            TaskAction::DeferTask(1),
            TaskAction::DeferTask(7),
            TaskAction::DeferTask(14),
        ]
    );
}

// --- TaskPalette: numbered rows (brain-menu parity) ---

#[test]
fn palette_rows_are_numbered_from_one_in_canonical_order() {
    // Numbers are the 1-based position in the scope-visible list, stable
    // regardless of the text filter — so the digit a user types always
    // points at the same command.
    let state = TaskPalette::new(
        Some("T1".into()),
        false,
        true,
        false,
        LinkKind::None,
        false,
        false,
    );
    let rows = state.rows();
    assert_eq!(rows[0].number, 1);
    assert_eq!(rows[1].number, 2);
    assert_eq!(rows.last().unwrap().number, rows.len());
}

#[test]
fn typing_a_row_number_filters_to_that_numbered_row() {
    // "2." prefixes the second command, so a query of "2" keeps it.
    let mut state = TaskPalette::new(
        Some("T1".into()),
        false,
        true,
        false,
        LinkKind::None,
        false,
        false,
    );
    let second = state.rows()[1].clone();
    state.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('2'),
        crossterm::event::KeyModifiers::NONE,
    ));
    let hits = state.visible();
    assert!(
        hits.iter().any(|c| c.action == second.action),
        "typing the row number should surface that numbered command"
    );
}
