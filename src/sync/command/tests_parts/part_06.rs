
#[test]
fn sync_plan_for_repair_names_each_slow_phase_up_front() {
    let cfg: SyncConfig =
        serde_json::from_str(r#"{"enabled":true,"b2_bucket":"bucket","b2_path":"brain-root"}"#)
            .unwrap();
    let plan = format_sync_plan(
        &cfg,
        Path::new("/tmp/brain"),
        Direction::Resync,
        Theme::dark(false),
    );

    assert!(plan.contains("Repairing cloud sync metadata"), "{plan}");
    assert!(plan.contains("local: /tmp/brain"), "{plan}");
    assert!(plan.contains("remote: BRAIN:bucket/brain-root"), "{plan}");
    assert!(!plan.contains("plan:"), "{plan}");
    assert!(!plan.contains("then:"), "{plan}");
}

#[test]
fn sync_progress_describes_each_direction_without_a_plan_block() {
    assert_eq!(
        sync_progress(Direction::Both),
        "Comparing local and remote changes, then syncing both directions…"
    );
    assert_eq!(
        sync_progress(Direction::Push),
        "Uploading local additions and edits without downloading remote changes…"
    );
}

#[test]
fn missing_rclone_guidance_names_both_install_commands() {
    let message = crate::sync::run::missing_rclone_guidance(Theme::dark(false), "brain sync");
    assert!(message.contains("rclone is not installed"), "{message}");
    assert!(
        message.contains("If you have Homebrew installed, use this option:"),
        "{message}"
    );
    assert!(
        message.contains("If you do not have Homebrew, use this option:"),
        "{message}"
    );
    assert!(message.contains("brew install rclone"), "{message}");
    assert!(
        message.contains("sudo -v ; curl https://rclone.org/install.sh | sudo bash"),
        "{message}"
    );
}

#[test]
fn csv_note_is_empty_when_nothing_changed() {
    assert_eq!(format_csv_note(&[]), "");
    assert_eq!(
        format_csv_note(&[crate::sync::csv_sync::CsvMergeOutcome::default()]),
        ""
    );
}
