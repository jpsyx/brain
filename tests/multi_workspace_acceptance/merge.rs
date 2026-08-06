use brain::sync::csv_merge::{SchemaStatus, merge, parse};

const FIRST_UUID: &str = "10000000-0000-4000-8000-000000000007";
const SECOND_UUID: &str = "20000000-0000-4000-8000-000000000007";
const HEADER: &str = "task_uuid,task_id,task_name,status,assigned_to,system_key,last_touched\n";

pub(crate) fn assert_independent_display_ids_converge() {
    let base = parse(HEADER, SchemaStatus::Current).expect("empty base");
    let personal = parse(
        &format!("{HEADER}{FIRST_UUID},T7,Personal task,not_started,pablo,,2026-08-06\n"),
        SchemaStatus::Current,
    )
    .expect("personal task table");
    let family = parse(
        &format!("{HEADER}{SECOND_UUID},T7,Family task,not_started,wife,,2026-08-06\n"),
        SchemaStatus::Current,
    )
    .expect("family task table");

    let merged = merge(&base, &personal, &family).0;
    let mirrored = merge(&base, &family, &personal).0;
    let task_id = merged
        .header
        .iter()
        .position(|column| column == "task_id")
        .expect("task ID column");

    assert_eq!(merged, mirrored);
    assert_eq!(merged.rows[FIRST_UUID][task_id], "T7");
    assert_eq!(merged.rows[SECOND_UUID][task_id], "T8");
}
