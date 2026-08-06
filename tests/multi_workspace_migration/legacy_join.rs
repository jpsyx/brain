use std::collections::BTreeMap;

use brain::workspace::{WorkspaceId, WorkspacePaths};

const WORKSPACE_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";
const REMOTE_SCHEMA: &str = r#"{"task_schema_version":2,"merge_key":"task_uuid","display_identity":{"field":"task_id","mutable":true}}"#;

#[test]
fn replayable_legacy_join_preserves_remote_uuids_and_never_publishes_legacy_shape() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("workspace");
    let tasks = root.join("tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    std::fs::write(
        tasks.join("tasks.csv"),
        "task_id,task_name,status,notes,assigned_to\n\
         T1,Shared,waiting,,pablo\n\
         T2,Local only,not_started,,pablo\n",
    )
    .unwrap();
    std::fs::write(
        tasks.join("habits.csv"),
        "task_id,task_name,status,notes,assigned_to\nH1,Walk,not_started,,pablo\n",
    )
    .unwrap();
    std::fs::write(tasks.join("SCHEMA.json"), "{}\n").unwrap();
    let paths = WorkspacePaths::new(temporary.path(), WorkspaceId::parse(WORKSPACE_ID).unwrap());
    std::fs::create_dir_all(paths.sync_csv_baselines()).unwrap();
    std::fs::write(
        paths.sync_csv_baselines().join("tasks.csv"),
        "task_id,task_name,status,notes,assigned_to\nT1,Shared,not_started,,pablo\n",
    )
    .unwrap();
    std::fs::write(
        paths.sync_csv_baselines().join("habits.csv"),
        "task_id,task_name,status,notes,assigned_to\nH1,Walk,not_started,,pablo\n",
    )
    .unwrap();
    let remote = BTreeMap::from([
        (
            "tasks/tasks.csv",
            "task_uuid,task_id,task_name,status,notes,assigned_to,system_key\n\
             10000000-0000-4000-8000-000000000001,T1,Shared,not_started,remote note,pablo,\n",
        ),
        (
            "tasks/habits.csv",
            "task_uuid,task_id,task_name,status,notes,assigned_to,system_key\n\
             20000000-0000-4000-8000-000000000001,H1,Walk,not_started,,pablo,\n",
        ),
    ]);

    for _ in 0..2 {
        brain::migration::join_legacy_to_current_with_transport(
            &paths,
            &root,
            REMOTE_SCHEMA,
            |relative| remote.get(relative).map(ToString::to_string),
        )
        .unwrap();
    }

    let rows = rows_by_task_id(&tasks.join("tasks.csv"));
    assert_eq!(
        rows["T1"]["task_uuid"],
        "10000000-0000-4000-8000-000000000001"
    );
    assert_eq!(rows["T1"]["status"], "waiting");
    assert_eq!(rows["T1"]["notes"], "remote note");
    assert_eq!(rows["T2"]["task_uuid"], "");
    assert_eq!(
        std::fs::read_to_string(tasks.join("SCHEMA.json")).unwrap(),
        "{}\n"
    );
    assert_eq!(
        remote["tasks/tasks.csv"],
        "task_uuid,task_id,task_name,status,notes,assigned_to,system_key\n\
         10000000-0000-4000-8000-000000000001,T1,Shared,not_started,remote note,pablo,\n"
    );
}

fn rows_by_task_id(path: &std::path::Path) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut reader = csv::Reader::from_path(path).unwrap();
    let headers = reader.headers().unwrap().clone();
    reader
        .records()
        .map(|record| {
            let record = record.unwrap();
            let row = headers
                .iter()
                .zip(record.iter())
                .map(|(column, value)| (column.to_owned(), value.to_owned()))
                .collect::<BTreeMap<_, _>>();
            (row["task_id"].clone(), row)
        })
        .collect()
}
