
#[test]
fn colored_suggestion_wraps_brain_sync_in_accent() {
    let t = Theme::dark(true);
    let push = vec!["a.md".to_string()];
    let report = format_report(&push, &[], &[], t);
    assert!(report.contains("\x1b[96mbrain sync\x1b[0m"), "{report:?}");
}

#[test]
fn csv_side_diff_counts_added_changed_and_deleted_rows() {
    let base = "task_id,title,status\n1,keep,open\n2,change,open\n3,delete,open\n";
    let side = "task_id,title,status\n1,keep,open\n2,changed,open\n4,add,open\n";

    assert_eq!(
        diff_csv_rows(base, side, SchemaStatus::Legacy).unwrap(),
        CsvSideDiff {
            added: 1,
            changed: 1,
            deleted: 1
        }
    );
}

#[test]
fn csv_diff_keys_by_uuid_and_aligns_reordered_headers() {
    let base = "task_uuid,task_id,status,notes\n\
                    10000000-0000-4000-8000-000000000010,T10,open,same\n";
    let side = "notes,status,task_id,task_uuid\n\
                    same,open,T10,10000000-0000-4000-8000-000000000010\n";

    assert_eq!(
        diff_csv_rows(base, side, SchemaStatus::Current).unwrap(),
        CsvSideDiff::default()
    );
}

#[test]
fn malformed_csv_diff_returns_a_typed_error() {
    let error = diff_csv_rows(
        "task_id,notes\nT1,ok\n",
        "task_id,notes\nT1,ok,unexpected\n",
        SchemaStatus::Legacy,
    )
    .unwrap_err();

    assert!(error.to_string().contains("malformed CSV record"));
    assert!(error.to_string().contains("row 2"));
}
