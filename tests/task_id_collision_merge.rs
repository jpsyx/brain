use brain::sync::csv_merge::{
    SchemaStatus, Table, merge, parse, project_task_lists, rewrite_project_metadata, serialize,
    validate_for_merge,
};

const BASE_UUID: &str = "00000000-0000-4000-8000-000000000009";
const LOCAL_PARENT_UUID: &str = "10000000-0000-4000-8000-000000000010";
const REMOTE_PARENT_UUID: &str = "20000000-0000-4000-8000-000000000010";
const LOCAL_CHILD_UUID: &str = "30000000-0000-4000-8000-000000000011";
const REMOTE_CHILD_UUID: &str = "40000000-0000-4000-8000-000000000012";

const HEADER: &str =
    "task_uuid,task_id,task_name,status,blocked_by,see_also,project,last_touched\n";

fn fixture() -> (Table, Table, Table) {
    let base = parse(
        &format!("{HEADER}{BASE_UUID},T9,Existing,not_started,,,,2026-08-01\n"),
        SchemaStatus::Current,
    )
    .unwrap();
    let local = parse(
        &format!(
            "{HEADER}\
         {BASE_UUID},T9,Existing,not_started,,,,2026-08-01\n\
         {LOCAL_PARENT_UUID},T10,Local child (1/2),not_started,,,alpha,2026-08-02\n\
         {LOCAL_CHILD_UUID},T11,Local child (2/2),waiting,T10|T9,T10,alpha,2026-08-02\n"
        ),
        SchemaStatus::Current,
    )
    .unwrap();
    let remote = parse(&format!(
        "{HEADER}\
         {BASE_UUID},T9,Existing,not_started,,,,2026-08-01\n\
         {REMOTE_PARENT_UUID},T10,Remote child (1/2),not_started,,,beta,2026-08-02\n\
         {REMOTE_CHILD_UUID},T12,Remote child (2/2),waiting,\"T10|T9,T10\",\"T10,https://example.test\",beta,2026-08-02\n"
    ), SchemaStatus::Current)
    .unwrap();
    (base, local, remote)
}

fn cell<'a>(table: &'a Table, uuid: &str, column: &str) -> &'a str {
    let index = table
        .header
        .iter()
        .position(|candidate| candidate == column)
        .expect("fixture column");
    &table.rows[uuid][index]
}

#[test]
fn collision_rewrites_composite_chunk_and_see_also_references() {
    let (base, local, remote) = fixture();

    let (merged, _) = merge(&base, &local, &remote);

    assert_eq!(merged.rows.len(), 5);
    assert_eq!(cell(&merged, LOCAL_PARENT_UUID, "task_id"), "T10");
    assert_eq!(cell(&merged, LOCAL_CHILD_UUID, "task_id"), "T11");
    assert_eq!(cell(&merged, REMOTE_CHILD_UUID, "task_id"), "T12");
    assert_eq!(cell(&merged, REMOTE_PARENT_UUID, "task_id"), "T13");
    assert_eq!(cell(&merged, LOCAL_CHILD_UUID, "blocked_by"), "T10|T9");
    assert_eq!(cell(&merged, REMOTE_CHILD_UUID, "blocked_by"), "T13|T9,T13");
    assert_eq!(cell(&merged, LOCAL_CHILD_UUID, "see_also"), "T10");
    assert_eq!(
        cell(&merged, REMOTE_CHILD_UUID, "see_also"),
        "T13,https://example.test"
    );
}

#[test]
fn deleted_reference_target_falls_back_to_original_display_id_without_marker_leak() {
    let header = "task_uuid,task_id,status,blocked_by,see_also,notes,last_touched\n";
    let base = parse(
        &format!(
            "{header}\
         {LOCAL_PARENT_UUID},T10,not_started,,,,2026-08-01\n\
         {LOCAL_CHILD_UUID},T11,waiting,T10,T10,original,2026-08-01\n"
        ),
        SchemaStatus::Current,
    )
    .unwrap();
    let local = parse(
        &format!(
            "{header}\
         {LOCAL_PARENT_UUID},T10,not_started,,,,2026-08-01\n\
         {LOCAL_CHILD_UUID},T11,waiting,T10,T10,edited,2026-08-02\n"
        ),
        SchemaStatus::Current,
    )
    .unwrap();
    let remote = parse(header, SchemaStatus::Current).unwrap();

    let merged = merge(&base, &local, &remote).0;

    assert!(!merged.rows.contains_key(LOCAL_PARENT_UUID));
    assert_eq!(cell(&merged, LOCAL_CHILD_UUID, "blocked_by"), "T10");
    assert_eq!(cell(&merged, LOCAL_CHILD_UUID, "see_also"), "T10");
    assert!(!serialize(&merged).contains("uuid:"));
}

#[test]
fn space_separated_see_also_rewrites_bounded_ids_without_touching_urls_or_substrings() {
    let header = "task_uuid,task_id,see_also,last_touched\n";
    let base = parse(header, SchemaStatus::Current).unwrap();
    let local = parse(
        &format!("{header}{LOCAL_PARENT_UUID},T10,,2026-08-02\n"),
        SchemaStatus::Current,
    )
    .unwrap();
    let remote = parse(&format!(
        "{header}\
         {REMOTE_PARENT_UUID},T10,,2026-08-02\n\
         {REMOTE_CHILD_UUID},T11,\"T10 https://linear.app/acme/issue/T10  (T10), T100 AT10 note-T10!\",2026-08-02\n"
    ), SchemaStatus::Current)
    .unwrap();

    let merged = merge(&base, &local, &remote).0;

    assert_eq!(cell(&merged, REMOTE_PARENT_UUID, "task_id"), "T12");
    assert_eq!(
        cell(&merged, REMOTE_CHILD_UUID, "see_also"),
        "T12 https://linear.app/acme/issue/T10  (T12), T100 AT10 note-T12!"
    );
}

#[test]
fn project_metadata_reverse_links_are_regenerated_from_final_display_ids() {
    let (base, local, remote) = fixture();
    let merged = merge(&base, &local, &remote).0;
    let projects = project_task_lists([&merged]);
    let metadata = br#"{"name":"beta","title":"Beta","tasks":["T10","T12"]}"#;

    let rewritten = rewrite_project_metadata(metadata, &projects["beta"]).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&rewritten).unwrap();

    assert_eq!(value["title"], "Beta");
    assert_eq!(value["tasks"], serde_json::json!(["T12", "T13"]));
}

#[test]
fn current_schema_requires_uuid_identity_columns_and_supported_version() {
    let valid = parse(
        &format!(
            "task_uuid,task_id,assigned_to,system_key,last_touched\n\
         {LOCAL_PARENT_UUID},T10,member-a,,2026-08-02\n"
        ),
        SchemaStatus::Current,
    )
    .unwrap();
    let missing_uuid = parse(
        "task_id,assigned_to,system_key,last_touched\nT10,member-a,,2026-08-02\n",
        SchemaStatus::Current,
    )
    .unwrap();
    let supported = r#"{"task_schema_version":2,"merge_key":"task_uuid"}"#;
    let unsupported = r#"{"task_schema_version":3,"merge_key":"task_uuid"}"#;

    assert!(validate_for_merge(Some(supported), &[&valid]).is_ok());
    assert!(
        validate_for_merge(Some(supported), &[&missing_uuid])
            .unwrap_err()
            .to_string()
            .contains("task_uuid")
    );
    assert!(
        validate_for_merge(Some(unsupported), &[&valid])
            .unwrap_err()
            .to_string()
            .contains("unsupported")
    );
}

#[test]
fn current_schema_accepts_identity_columns_without_last_touched() {
    let table = parse(
        &format!(
            "task_uuid,task_id,assigned_to,system_key\n\
         {LOCAL_PARENT_UUID},T10,member-a,\n"
        ),
        SchemaStatus::Current,
    )
    .unwrap();
    let supported = r#"{"task_schema_version":2,"merge_key":"task_uuid"}"#;

    assert!(validate_for_merge(Some(supported), &[&table]).is_ok());
}

#[test]
fn nonempty_legacy_table_without_task_id_is_rejected() {
    let table = parse(
        "title,status\nMissing identity,not_started\n",
        SchemaStatus::Legacy,
    )
    .unwrap();

    for manifest in [None, Some("{}")] {
        let error = validate_for_merge(manifest, &[&table]).unwrap_err();
        assert!(error.to_string().contains("task_id"));
    }
}

#[test]
fn unknown_columns_require_explicit_forward_compatibility() {
    let table = parse(
        &format!(
            "task_uuid,task_id,assigned_to,system_key,last_touched,future_field\n\
         {LOCAL_PARENT_UUID},T10,member-a,,2026-08-02,preserve me\n"
        ),
        SchemaStatus::Current,
    )
    .unwrap();
    let strict = r#"{"task_schema_version":2,"merge_key":"task_uuid"}"#;
    let compatible =
        r#"{"task_schema_version":2,"merge_key":"task_uuid","forward_compatible_columns":true}"#;

    assert!(
        validate_for_merge(Some(strict), &[&table])
            .unwrap_err()
            .to_string()
            .contains("future_field")
    );
    assert!(validate_for_merge(Some(compatible), &[&table]).is_ok());
}

#[test]
fn collision_reconciliation_is_mirror_order_independent_and_idempotent() {
    let (base, local, remote) = fixture();

    let first = merge(&base, &local, &remote).0;
    let mirrored = merge(&base, &remote, &local).0;
    let repeated = merge(&first, &first, &first).0;

    assert_eq!(serialize(&first), serialize(&mirrored));
    assert_eq!(serialize(&first), serialize(&repeated));
}

#[test]
fn mirror_order_is_independent_of_forward_compatible_header_order() {
    let base = parse(
        &format!("task_uuid,task_id,last_touched\n{BASE_UUID},T9,2026-08-01\n"),
        SchemaStatus::Current,
    )
    .unwrap();
    let local = parse(
        &format!(
            "task_uuid,task_id,local_field,last_touched\n\
         {LOCAL_PARENT_UUID},T10,local,2026-08-02\n"
        ),
        SchemaStatus::Current,
    )
    .unwrap();
    let remote = parse(
        &format!(
            "remote_field,last_touched,task_id,task_uuid\n\
         remote,2026-08-02,T11,{REMOTE_PARENT_UUID}\n"
        ),
        SchemaStatus::Current,
    )
    .unwrap();

    let first = merge(&base, &local, &remote).0;
    let mirrored = merge(&base, &remote, &local).0;

    assert_eq!(serialize(&first), serialize(&mirrored));
}
