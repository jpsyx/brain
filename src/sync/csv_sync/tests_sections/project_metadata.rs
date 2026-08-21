
#[test]
fn listed_remote_schema_read_failure_is_not_treated_as_legacy_absence() {
    let directory = tempfile::tempdir().unwrap();
    let mut step = 0;

    let error = fetch_remote_task_schema_with("BRAIN:shared/brain", directory.path(), |args| {
        let response = match step {
            // A `tasks/`-scoped listing returns bare names.
            0 => (true, "SCHEMA.json\n".to_owned()),
            1 => (false, "remote read failed".to_owned()),
            _ => panic!("unexpected remote schema command: {args:?}"),
        };
        step += 1;
        response
    })
    .unwrap_err();

    assert!(
        error.to_string().contains("remote read failed"),
        "{error:#}"
    );
}

#[test]
fn reconciled_project_metadata_is_written_and_pushed_with_final_ids() {
    use std::cell::RefCell;

    let directory = tempfile::tempdir().unwrap();
    let metadata = directory.path().join("projects/alpha/.METADATA.json");
    std::fs::create_dir_all(metadata.parent().unwrap()).unwrap();
    std::fs::write(
        &metadata,
        b"{\"name\":\"alpha\",\"title\":\"Alpha\",\"tasks\":[\"T10\"]}\n",
    )
    .unwrap();
    let table = parse(
        "task_uuid,task_id,project\n\
             10000000-0000-4000-8000-000000000010,T10,alpha\n\
             20000000-0000-4000-8000-000000000010,T13,alpha\n",
        crate::sync::csv_merge::SchemaStatus::Current,
    )
    .unwrap();
    let pushed = RefCell::new(Vec::new());

    let changed = reconcile_project_metadata(directory.path(), &[table], true, |relative, text| {
        pushed
            .borrow_mut()
            .push((relative.to_owned(), text.to_owned()));
        true
    })
    .unwrap();

    let local: serde_json::Value =
        serde_json::from_slice(&std::fs::read(metadata).unwrap()).unwrap();
    assert_eq!(changed, 1);
    assert_eq!(local["title"], "Alpha");
    assert_eq!(local["tasks"], serde_json::json!(["T10", "T13"]));
    assert_eq!(pushed.borrow().len(), 1);
    assert_eq!(pushed.borrow()[0].0, "projects/alpha/.METADATA.json");
}

#[test]
fn malformed_project_metadata_aborts_before_rewriting_unrelated_projects() {
    let directory = tempfile::tempdir().unwrap();
    let alpha = directory.path().join("projects/alpha/.METADATA.json");
    let broken = directory.path().join("projects/zeta/.METADATA.json");
    std::fs::create_dir_all(alpha.parent().unwrap()).unwrap();
    std::fs::create_dir_all(broken.parent().unwrap()).unwrap();
    let original = b"{\"name\":\"alpha\",\"tasks\":[\"T10\"]}\n";
    std::fs::write(&alpha, original).unwrap();
    std::fs::write(&broken, b"not json\n").unwrap();
    let table = parse(
        "task_uuid,task_id,project\n\
             10000000-0000-4000-8000-000000000010,T13,alpha\n",
        crate::sync::csv_merge::SchemaStatus::Current,
    )
    .unwrap();

    let result = reconcile_project_metadata(directory.path(), &[table], true, |_, _| true);

    assert!(result.is_err());
    assert_eq!(std::fs::read(alpha).unwrap(), original);
}

#[cfg(unix)]
#[test]
fn project_metadata_local_write_failure_is_classified_as_local_write() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().unwrap();
    let metadata = directory.path().join("projects/alpha/.METADATA.json");
    std::fs::create_dir_all(metadata.parent().unwrap()).unwrap();
    std::fs::write(&metadata, b"{\"name\":\"alpha\",\"tasks\":[\"T99\"]}\n").unwrap();
    std::fs::set_permissions(&metadata, std::fs::Permissions::from_mode(0o400)).unwrap();
    let paths = paths(directory.path());

    let result = sync_csvs_with_transport(
        &paths,
        directory.path(),
        Direction::Both,
        |_| None,
        |_, _| true,
    );

    std::fs::set_permissions(&metadata, std::fs::Permissions::from_mode(0o600)).unwrap();
    let error = result.unwrap_err();
    assert!(matches!(
        &error,
        CsvSyncError::LocalWrite(message) if message.contains(".METADATA.json")
    ));
    assert!(
        error
            .to_string()
            .starts_with("task state local write failed: writing project metadata")
    );
}
