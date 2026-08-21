
#[test]
fn format_triggers_shows_debounce_window_when_watch_on() {
    let cfg: SyncConfig = serde_json::from_str(r#"{"enabled":true,"b2_bucket":"b"}"#).unwrap();
    let line = format_triggers(&cfg, Theme::dark(false));
    assert!(line.contains("change-push on"), "{line}");
    assert!(line.contains("3000ms"), "{line}");
}
#[test]
fn format_triggers_hides_debounce_window_when_watch_off() {
    // watch is disabled → the debounce window is meaningless, so don't show it.
    let cfg: SyncConfig =
        serde_json::from_str(r#"{"enabled":true,"b2_bucket":"b","watch":false}"#).unwrap();
    let line = format_triggers(&cfg, Theme::dark(false));
    assert!(line.contains("change-push off"), "{line}");
    assert!(!line.contains("debounce"), "{line}");
}

#[test]
fn format_triggers_advertises_the_five_minute_periodic_pull() {
    let cfg: SyncConfig = serde_json::from_str(r#"{"enabled":true,"b2_bucket":"b"}"#).unwrap();
    let line = format_triggers(&cfg, Theme::dark(false));
    assert!(line.contains("periodic-pull every 5m"), "{line}");
}

#[test]
fn format_triggers_colors_on_and_off_flags() {
    let cfg: SyncConfig =
        serde_json::from_str(r#"{"enabled":true,"b2_bucket":"b","watch":false}"#).unwrap();
    let s = format_triggers(&cfg, Theme::dark(true));
    assert!(
        s.contains("\x1b[92m"),
        "on flags should be success green: {s}"
    );
    assert!(
        s.contains("\x1b[90m"),
        "off flags should be muted gray: {s}"
    );
}
