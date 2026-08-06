
#[test]
fn format_in_progress_names_the_running_direction_and_start() {
    let state = crate::sync::current::CurrentState {
        pid: 4242,
        direction: "both".into(),
        started_at: "2026-07-29T01:00:00Z".into(),
    };
    let line = format_in_progress(&state, Theme::dark(false));
    assert!(line.contains("syncing now"), "{line}");
    assert!(line.contains("both"), "{line}");
    assert!(line.contains("2026-07-29T01:00:00Z"), "{line}");
    assert!(line.contains("pid 4242"), "{line}");
}

#[test]
fn format_last_run_handles_empty_and_populated() {
    let theme = Theme::dark(false);
    assert!(format_last_run(None, theme).contains("no syncs yet"));
    let r = crate::sync::journal::SyncRun {
        started_at: "s".into(),
        finished_at: "2026-07-25T00:00:05Z".into(),
        direction: "both".into(),
        outcome: "clean".into(),
        transferred: 3,
        deleted: 1,
        conflicts: 0,
        errors: 0,
        note: String::new(),
    };
    let line = format_last_run(Some(&r), theme);
    assert!(line.contains("both") && line.contains("clean") && line.contains("3↑"));
}

#[test]
fn format_last_run_colors_the_outcome_by_value() {
    let clean_run = crate::sync::journal::SyncRun {
        started_at: "s".into(),
        finished_at: "2026-07-25T00:00:05Z".into(),
        direction: "both".into(),
        outcome: "clean".into(),
        transferred: 3,
        deleted: 1,
        conflicts: 0,
        errors: 0,
        note: String::new(),
    };
    let line = format_last_run(Some(&clean_run), Theme::dark(true));
    assert!(
        line.contains("\x1b[92m"),
        "clean outcome should be colored success green: {line}"
    );

    let aborted_run = crate::sync::journal::SyncRun {
        outcome: "aborted".into(),
        ..clean_run
    };
    let line = format_last_run(Some(&aborted_run), Theme::dark(true));
    assert!(
        line.contains("\x1b[91m"),
        "aborted outcome should be colored error red: {line}"
    );
}

#[test]
fn format_triggers_keeps_startup_pull_on_when_legacy_config_disables_it() {
    let cfg: SyncConfig =
        serde_json::from_str(r#"{"enabled":true,"b2_bucket":"b","on_start":false}"#).unwrap();
    let s = format_triggers(&cfg, Theme::dark(false));
    assert!(s.contains("startup-pull on"), "{s}");
    assert!(s.contains("change-push on"), "{s}");
    assert!(s.contains("message-pull after 2h"), "{s}");
}
