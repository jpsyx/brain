//! Identical persisted inputs and hand-derived after-images for CLI/TUI acceptance.
use std::{fs, path::Path};

pub const CSV: &str = "task_uuid,task_id,task_name,status,blocked_by,last_touched\n10000000-0000-4000-8000-000000000001,T1,First,not_started,,2026-10-14\n10000000-0000-4000-8000-000000000002,T2,Middle,not_started,T1,2026-10-14\n10000000-0000-4000-8000-000000000003,T3,Last,not_started,T2,2026-10-14\n";
pub const METADATA: &str = "archive/projects/sample__outcome/.METADATA.json";
pub const AGENDA: &str = "## ❗ MIT\n\n- [ ] ❗ **T2** Middle\n- [ ] ❗ **T3** Last\n";

pub fn seed(root: &Path, agenda: &Path) {
    fs::write(root.join("tasks/tasks.csv"), CSV).unwrap();
    fs::write(root.join("tasks/habits.csv"), "task_id,task_name\n").unwrap();
    let metadata = root.join(METADATA);
    fs::create_dir_all(metadata.parent().unwrap()).unwrap();
    fs::write(metadata, r#"{"name":"sample__outcome","namespace":"sample","title":"Example outcome","status":"archived","priority":"p2","due":"none","directory":"archive/projects/sample__outcome","tasks":["T1","T2","T3"],"extra_reference":"preserve"}"#).unwrap();
    fs::create_dir_all(agenda.parent().unwrap()).unwrap();
    fs::write(agenda, AGENDA).unwrap();
}

pub fn images(root: &Path, agenda: &Path) -> Vec<Vec<u8>> {
    [
        root.join("tasks/tasks.csv"),
        root.join("tasks/habits.csv"),
        root.join(METADATA),
        agenda.to_owned(),
    ]
    .into_iter()
    .map(|path| fs::read(path).unwrap())
    .collect()
}

pub fn assert_after(root: &Path, agenda: &Path, date: &str) {
    let csv = fs::read_to_string(root.join("tasks/tasks.csv")).unwrap();
    assert_eq!(
        csv,
        format!(
            "task_uuid,task_id,task_name,status,blocked_by,last_touched\n10000000-0000-4000-8000-000000000001,T1,First,not_started,,2026-10-14\n10000000-0000-4000-8000-000000000003,T3,Last,not_started,T1,{date}\n"
        )
    );
    let metadata: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join(METADATA)).unwrap()).unwrap();
    assert_eq!(
        metadata,
        serde_json::json!({"name":"sample__outcome","namespace":"sample","title":"Example outcome","status":"archived","priority":"p2","due":"none","directory":"archive/projects/sample__outcome","tasks":["T1","T3"],"extra_reference":"preserve"})
    );
    assert_eq!(
        fs::read_to_string(root.join("tasks/habits.csv")).unwrap(),
        "task_id,task_name\n"
    );
    assert_eq!(
        fs::read_to_string(agenda).unwrap(),
        "## ❗ MIT\n\n- [ ] ❗ **T3** Last\n"
    );
}
