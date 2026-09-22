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
fn the_hidden_files_row_names_the_flip_that_will_happen_next() {
    let out_of_sight = palette(&shared_workspace());
    let on_screen = palette(&PaletteContext {
        show_hidden_files: true,
        ..shared_workspace()
    });

    assert_eq!(
        label_of(
            &out_of_sight,
            Command::Global(GlobalAction::ToggleHiddenFiles)
        )
        .as_deref(),
        Some("Show hidden files")
    );
    assert_eq!(
        label_of(&on_screen, Command::Global(GlobalAction::ToggleHiddenFiles)).as_deref(),
        Some("Hide hidden files")
    );
}

#[test]
fn the_file_explorer_row_reads_the_same_whatever_is_highlighted() {
    // It takes no target, so unlike the Explore row it never names one.
    let bare = palette(&shared_workspace());
    let with_an_entry = palette(&with_entry(entry("plan.md", true, true)));

    for state in [&bare, &with_an_entry] {
        assert_eq!(
            label_of(state, Command::Global(GlobalAction::OpenFileExplorer)).as_deref(),
            Some("Open the file explorer at the brain root")
        );
    }
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
