#[test]
fn the_receiver_row_names_the_next_persistent_action() {
    let off = palette(&shared_workspace());
    let on = palette(&PaletteContext {
        receiver_enabled: true,
        ..shared_workspace()
    });

    assert_eq!(
        label_of(&off, Command::Global(GlobalAction::ToggleReceiver)).as_deref(),
        Some("Enable receiver")
    );
    assert_eq!(
        label_of(&on, Command::Global(GlobalAction::ToggleReceiver)).as_deref(),
        Some("Disable receiver")
    );
}

#[test]
fn the_daily_triage_row_names_the_next_action_and_is_always_offered() {
    let armed = palette(&shared_workspace());
    let silenced = palette(&PaletteContext {
        daily_triage_alert_disabled: true,
        ..shared_workspace()
    });

    assert_eq!(
        label_of(&armed, Command::Global(GlobalAction::ToggleDailyTriageAlert)).as_deref(),
        Some("Disable daily triage alert")
    );
    assert_eq!(
        label_of(
            &silenced,
            Command::Global(GlobalAction::ToggleDailyTriageAlert)
        )
        .as_deref(),
        Some("Enable daily triage alert")
    );
}

#[test]
fn the_layout_row_names_the_direction_the_panel_would_move() {
    let right = palette(&shared_workspace());
    let left = palette(&PaletteContext {
        panel_side: crate::state::PanelSide::Left,
        ..shared_workspace()
    });

    assert_eq!(
        label_of(&right, Command::Global(GlobalAction::ToggleLayout)).as_deref(),
        Some("Move brain panel to the left")
    );
    assert_eq!(
        label_of(&left, Command::Global(GlobalAction::ToggleLayout)).as_deref(),
        Some("Move brain panel to the right")
    );
}
