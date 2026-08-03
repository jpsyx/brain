use brain::sync::csv_merge::{
    Table, merge, parse, project_task_lists, rewrite_project_metadata, serialize,
    validate_for_merge,
};

const BASE_UUID: &str = "00000000-0000-4000-8000-000000000009";
const LOCAL_PARENT_UUID: &str = "10000000-0000-4000-8000-000000000010";
const REMOTE_PARENT_UUID: &str = "20000000-0000-4000-8000-000000000010";
const LOCAL_CHILD_UUID: &str = "30000000-0000-4000-8000-000000000011";
const REMOTE_CHILD_UUID: &str = "40000000-0000-4000-8000-000000000012";

const HEADER: &str = "task_uuid,task_id,task_name,status,blocked_by,project,last_touched\n";

fn fixture() -> (Table, Table, Table) {
    let base = parse(&format!(
        "{HEADER}{BASE_UUID},T9,Existing,not_started,,,2026-08-01\n"
    ));
    let local = parse(&format!(
        "{HEADER}\
         {BASE_UUID},T9,Existing,not_started,,,2026-08-01\n\
         {LOCAL_PARENT_UUID},T10,Local parent,not_started,,alpha,2026-08-02\n\
         {LOCAL_CHILD_UUID},T11,Local child,waiting,T10|T9,alpha,2026-08-02\n"
    ));
    let remote = parse(&format!(
        "{HEADER}\
         {BASE_UUID},T9,Existing,not_started,,,2026-08-01\n\
         {REMOTE_PARENT_UUID},T10,Remote parent,not_started,,beta,2026-08-02\n\
         {REMOTE_CHILD_UUID},T12,Remote child,waiting,\"T10|T9,T10\",beta,2026-08-02\n"
    ));
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
fn distinct_uuid_rows_survive_display_id_collision_and_rewrite_blocked_by() {
    let (base, local, remote) = fixture();

    let (merged, _) = merge(&base, &local, &remote);

    assert_eq!(merged.rows.len(), 5);
    assert_eq!(cell(&merged, LOCAL_PARENT_UUID, "task_id"), "T10");
    assert_eq!(cell(&merged, LOCAL_CHILD_UUID, "task_id"), "T11");
    assert_eq!(cell(&merged, REMOTE_CHILD_UUID, "task_id"), "T12");
    assert_eq!(cell(&merged, REMOTE_PARENT_UUID, "task_id"), "T13");
    assert_eq!(cell(&merged, LOCAL_CHILD_UUID, "blocked_by"), "T10|T9");
    assert_eq!(cell(&merged, REMOTE_CHILD_UUID, "blocked_by"), "T13|T9,T13");
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
    let valid = parse(&format!(
        "task_uuid,task_id,assigned_to,system_key,last_touched\n\
         {LOCAL_PARENT_UUID},T10,member-a,,2026-08-02\n"
    ));
    let missing_uuid =
        parse("task_id,assigned_to,system_key,last_touched\nT10,member-a,,2026-08-02\n");
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
fn unknown_columns_require_explicit_forward_compatibility() {
    let table = parse(&format!(
        "task_uuid,task_id,assigned_to,system_key,last_touched,future_field\n\
         {LOCAL_PARENT_UUID},T10,member-a,,2026-08-02,preserve me\n"
    ));
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
    let base = parse(&format!(
        "task_uuid,task_id,last_touched\n{BASE_UUID},T9,2026-08-01\n"
    ));
    let local = parse(&format!(
        "task_uuid,task_id,local_field,last_touched\n\
         {LOCAL_PARENT_UUID},T10,local,2026-08-02\n"
    ));
    let remote = parse(&format!(
        "remote_field,last_touched,task_id,task_uuid\n\
         remote,2026-08-02,T11,{REMOTE_PARENT_UUID}\n"
    ));

    let first = merge(&base, &local, &remote).0;
    let mirrored = merge(&base, &remote, &local).0;

    assert_eq!(serialize(&first), serialize(&mirrored));
}
