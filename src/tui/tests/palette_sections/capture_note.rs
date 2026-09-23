#[test]
fn creating_a_capture_note_is_a_listed_global_command_with_no_shortcut() {
    let listed = palette(&shared_workspace());

    assert_eq!(
        label_of(&listed, Command::Global(GlobalAction::CreateCaptureNote)).as_deref(),
        Some("Create a new capture note")
    );
    assert_eq!(
        shortcut_for(Command::Global(GlobalAction::CreateCaptureNote)),
        None
    );
}
