use std::path::Path;

use super::*;
use crate::tasks::schema::columns::is_known_current_column;

const TASKS_HEADER: &str = "task_uuid,task_id,task_name,task_type,status,waiting_since,priority,due_date,hard_deadline,start_date,assigned_to,see_also,notes,project,energy_level,context,estimated_duration,blocked_by,defer_count,created_date,completed_date,last_touched,linear_issue,system_key\n";
const HABITS_HEADER: &str = "task_uuid,task_id,task_name,status,priority,due_date,hard_deadline,assigned_to,see_also,notes,project,energy_level,context,estimated_duration,ideal_time,recur_interval,recur_unit,created_date,completed_date,last_touched,system_key\n";

fn seeded_workspace() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("tasks")).unwrap();
    std::fs::write(root.path().join("tasks/tasks.csv"), TASKS_HEADER).unwrap();
    std::fs::write(root.path().join("tasks/habits.csv"), HABITS_HEADER).unwrap();
    root
}

fn documented_columns(table: &str) -> Vec<String> {
    let schema: serde_json::Value = serde_json::from_str(CANONICAL_DOCUMENT).unwrap();
    schema[table]["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|column| column["name"].as_str().unwrap().to_owned())
        .collect()
}

fn header_columns(header: &str) -> Vec<String> {
    header.trim().split(',').map(str::to_owned).collect()
}

#[test]
fn the_canonical_document_declares_the_current_schema() {
    let schema: serde_json::Value = serde_json::from_str(CANONICAL_DOCUMENT).unwrap();

    assert_eq!(
        schema["task_schema_version"].as_u64(),
        Some(crate::tasks::schema::TASK_SCHEMA_VERSION)
    );
    assert_eq!(schema["merge_key"].as_str(), Some("task_uuid"));
    assert_eq!(schema["display_identity"]["field"].as_str(), Some("task_id"));
    assert_eq!(
        schema["display_identity"]["mutable"].as_bool(),
        Some(true)
    );
    assert_eq!(
        schema["forward_compatible_columns"].as_bool(),
        Some(true)
    );
}

/// The `~/brain` schema this replaced documented `assignee` long after the CSVs
/// moved to `assigned_to`, and omitted `task_uuid` entirely. Documentation that
/// drifts from the columns Brain actually writes is worse than none.
#[test]
fn the_canonical_document_documents_exactly_the_seeded_columns() {
    assert_eq!(documented_columns("tasks_csv"), header_columns(TASKS_HEADER));
    assert_eq!(
        documented_columns("habits_csv"),
        header_columns(HABITS_HEADER)
    );
    for table in ["tasks_csv", "habits_csv"] {
        for column in documented_columns(table) {
            assert!(
                is_known_current_column(&column),
                "{table} documents unknown column {column}"
            );
        }
    }
}

#[test]
fn the_canonical_document_carries_no_personal_data() {
    let lowercase = CANONICAL_DOCUMENT.to_lowercase();
    for forbidden in [
        "pablo",
        "avandar",
        "~/brain/",
        "/users/",
        "ava-",
        "zotero",
        "notion",
    ] {
        assert!(
            !lowercase.contains(forbidden),
            "the bundled schema must stay generic; found {forbidden}"
        );
    }
}

#[test]
fn a_workspace_missing_the_document_is_seeded_and_reads_as_current() {
    let root = seeded_workspace();

    assert!(!document_present(root.path()));
    assert!(ensure_schema_document(root.path()).unwrap());
    assert!(document_present(root.path()));

    let inspection = crate::tasks::schema::inspect_inactive(root.path()).unwrap();
    assert_eq!(
        inspection.version,
        Some(crate::tasks::schema::TASK_SCHEMA_VERSION)
    );
    assert!(
        inspection.current,
        "a freshly seeded workspace must read as schema-current"
    );
}

#[test]
fn seeding_never_overwrites_an_existing_document() {
    let root = seeded_workspace();
    let path = root.path().join("tasks/SCHEMA.json");
    std::fs::write(&path, b"{\"task_schema_version\": 2, \"mine\": true}\n").unwrap();

    assert!(!ensure_schema_document(root.path()).unwrap());
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"{\"task_schema_version\": 2, \"mine\": true}\n"
    );
}

#[test]
fn seeding_is_idempotent() {
    let root = seeded_workspace();

    assert!(ensure_schema_document(root.path()).unwrap());
    let first = std::fs::read(root.path().join("tasks/SCHEMA.json")).unwrap();
    assert!(!ensure_schema_document(root.path()).unwrap());

    assert_eq!(
        std::fs::read(root.path().join("tasks/SCHEMA.json")).unwrap(),
        first
    );
}

/// `brain workspace migrate` on an uninitialized store reported only a path.
#[test]
fn an_uninitialized_task_store_says_so_instead_of_naming_a_path() {
    let root = tempfile::tempdir().unwrap();

    let error = crate::tasks::schema::inspect_inactive(root.path()).unwrap_err();

    let message = error.to_string();
    assert!(message.contains("not initialized"), "{message}");
    assert!(message.contains("tasks.csv"), "{message}");
    assert!(message.contains("brain tasks today"), "{message}");
}

#[test]
fn a_workspace_with_no_tasks_directory_is_left_alone() {
    let root = tempfile::tempdir().unwrap();

    assert!(!ensure_schema_document(root.path()).unwrap());
    assert!(!document_present(root.path()));
}

#[test]
fn document_presence_is_reported_for_the_health_check() {
    let root = seeded_workspace();
    assert!(!document_present(root.path()));
    ensure_schema_document(root.path()).unwrap();
    assert!(document_present(Path::new(root.path())));
}
