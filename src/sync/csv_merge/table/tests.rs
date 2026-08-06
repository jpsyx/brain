use super::{SchemaStatus, parse, remote_schema_status};

#[test]
fn present_remote_schema_requires_a_typed_complete_supported_manifest() {
    let invalid = [
        "{}",
        r#"{"merge_key":"task_uuid"}"#,
        r#"{"task_schema_version":"3","merge_key":"task_uuid"}"#,
        r#"{"task_schema_version":2}"#,
        r#"{"task_schema_version":2,"merge_key":3}"#,
        r#"{"task_schema_version":3,"merge_key":"task_uuid"}"#,
        r#"{"task_schema_version":2,"merge_key":"task_uuid"}"#,
        r#"{"task_schema_version":2,"merge_key":"task_uuid","display_identity":{"field":"task_id"}}"#,
        r#"{"task_schema_version":2,"merge_key":"task_uuid","display_identity":{"field":"task_id","mutable":"true"}}"#,
    ];

    assert_eq!(remote_schema_status(None).unwrap(), SchemaStatus::Legacy);
    for manifest in invalid {
        assert!(
            remote_schema_status(Some(manifest)).is_err(),
            "present remote schema was accepted: {manifest}"
        );
    }
    assert_eq!(
            remote_schema_status(Some(
                r#"{"task_schema_version":2,"merge_key":"task_uuid","display_identity":{"field":"task_id","mutable":true}}"#
            ))
            .unwrap(),
            SchemaStatus::Current
        );
}

#[test]
fn known_legacy_remote_schema_is_legacy() {
    let legacy = r#"{
            "tasks_csv": {"key": "task_id", "columns": []},
            "habits_csv": {"key": "task_id", "columns": []}
        }"#;

    assert_eq!(
        remote_schema_status(Some(legacy)).unwrap(),
        SchemaStatus::Legacy
    );
}

#[test]
fn hybrid_legacy_rows_remain_keyed_by_task_id() {
    let table = parse(
        "task_id,task_uuid,status\n\
             T1,,not_started\n\
             T2,10000000-0000-4000-8000-000000000002,not_started\n",
        SchemaStatus::Legacy,
    )
    .unwrap();

    assert_eq!(table.merge_key(), Some("task_id"));
    assert_eq!(table.rows.len(), 2);
    assert!(table.rows.contains_key("T1"));
    assert!(table.rows.contains_key("T2"));
}

#[test]
fn duplicate_current_task_uuid_is_rejected() {
    let error = parse(
        "task_uuid,task_id\n\
             10000000-0000-4000-8000-000000000001,T1\n\
             10000000-0000-4000-8000-000000000001,T2\n",
        SchemaStatus::Current,
    )
    .unwrap_err();

    assert!(error.to_string().contains("duplicate task_uuid"));
    assert!(error.to_string().contains("row 3"));
}

#[test]
fn duplicate_legacy_task_id_is_rejected() {
    let error = parse(
        "task_id,status\nT1,not_started\nT1,done\n",
        SchemaStatus::Legacy,
    )
    .unwrap_err();

    assert!(error.to_string().contains("duplicate task_id"));
    assert!(error.to_string().contains("row 3"));
}

#[test]
fn malformed_csv_record_is_rejected_with_its_row() {
    let error = parse(
        "task_id,notes\nT1,ok\nT2,ok,unexpected\n",
        SchemaStatus::Legacy,
    )
    .unwrap_err();

    assert!(error.to_string().contains("malformed CSV record"));
    assert!(error.to_string().contains("row 3"));
}
