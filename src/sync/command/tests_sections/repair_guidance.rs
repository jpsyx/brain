
#[test]
fn sync_repair_before_setup_points_to_setup() {
    let message = format_unconfigured_sync_guidance(Direction::Resync, Theme::dark(false));

    assert!(
        message.contains("Cloud sync is not set up yet."),
        "{message}"
    );
    assert!(
        message.contains("`brain sync repair` only repairs an existing sync setup"),
        "{message}"
    );
    assert!(message.contains("Run `brain sync setup`."), "{message}");
}
