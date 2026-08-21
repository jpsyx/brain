#[test]
fn shell_has_one_overlay_owner_and_picker_has_none() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let tui = std::fs::read_to_string(root.join("src/tui/mod.rs")).expect("read TUI shell");
    let picker = std::fs::read_to_string(root.join("src/picker/mod.rs")).expect("read picker");
    let modal_route = std::fs::read_to_string(root.join("src/tui/event_loop/modal_route.rs"))
        .expect("read modal route");

    assert!(
        tui.contains("overlay: Option<Overlay>"),
        "the shell must own exactly one data-bearing overlay slot"
    );
    assert_eq!(
        tui.matches("Option<Overlay>").count(),
        1,
        "the shell must expose only one overlay slot"
    );
    assert!(
        !modal_route.contains("ActiveModals"),
        "the precedence-booleans snapshot must not remain"
    );
    for independent_slot in [
        "palette: Option<PaletteState>",
        "brain_input: Option<BrainInputState>",
        "confirm: Option<ConfirmState>",
        "link_picker: Option<LinkPickerState>",
        "assignee_filter: Option<AssigneeFilterState>",
        "help: Option<HelpState>",
        "sync_log: Option<SyncLogState>",
    ] {
        assert!(
            !tui.contains(independent_slot),
            "the shell still exposes independent modal slot {independent_slot}"
        );
    }
    assert!(
        !picker.contains("Option<menu::MenuApp>"),
        "picker::App must not own the search palette"
    );
    assert!(
        !picker.contains("Option<Confirm>"),
        "picker::App must not own search confirmation state"
    );
}
