
#[test]
fn baseline_read_failure_is_labeled_instead_of_treated_as_empty() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
    std::fs::write(
        dir.path().join("tasks/tasks.csv"),
        "task_id,status\nT1,open\n",
    )
    .unwrap();

    let error = collect_csv_pending_with_fetch(
        dir.path(),
        &["tasks/tasks.csv"],
        |_| Err("permission denied".to_owned()),
        |_| None,
    )
    .unwrap_err();

    assert!(error.to_string().contains("baseline tasks/tasks.csv"));
    assert!(error.to_string().contains("permission denied"));
}

#[test]
fn read_only_check_ignores_timestamp_only_drift_without_changing_real_sync_policy() {
    let cfg: SyncConfig =
        serde_json::from_str(r#"{"enabled":true,"b2_bucket":"bucket"}"#).unwrap();
    let check = check_bisync_args(&cfg, "/brain", "remote:brain", "/work");
    let sync = crate::sync::args::bisync_args(
        &cfg,
        "/brain",
        "remote:brain",
        "/work",
        crate::sync::args::Direction::Both,
    );

    assert!(
        check
            .windows(2)
            .any(|pair| pair == ["--compare", "size,checksum"])
    );
    assert!(check.iter().any(|arg| arg == "--dry-run"));
    assert!(!sync.iter().any(|arg| arg == "--compare"));
}
