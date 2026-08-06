
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
