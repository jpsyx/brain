//! Deletion reconciliation acceptance tests with an in-memory remote.
use std::cell::RefCell;
use std::collections::BTreeMap;

use brain::sync::args::Direction;
use brain::sync::csv_sync::sync_csvs_with_transport;
use brain::workspace::{WorkspaceContext, WorkspaceId, WorkspaceName};

const HEADER: &str = "task_uuid,task_id,task_name,project,assigned_to,system_key,last_touched\n";
const SCHEMA: &str = r#"{"task_schema_version":2,"merge_key":"task_uuid","display_identity":{"field":"task_id","mutable":true}}"#;

#[test]
fn local_deletion_wins_against_unchanged_remote_row() {
    reconcile(false);
}

#[test]
fn edited_remote_row_survives_local_deletion_with_conflict() {
    reconcile(true);
}

fn reconcile(edited: bool) {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("workspace");
    let workspace = WorkspaceContext::new(
        temporary.path(),
        WorkspaceId::new(),
        WorkspaceName::parse("sample").unwrap(),
        &root,
        "member",
        temporary.path(),
    )
    .unwrap();
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    let baseline = format!(
        "{HEADER}10000000-0000-4000-8000-000000000001,T1,Original,work__sample,member,,2026-10-01\n"
    );
    let remote_row = if edited {
        baseline
            .replace("Original", "Edited")
            .replace("2026-10-01", "2026-10-02")
    } else {
        baseline.clone()
    };
    for path in ["tasks/tasks.csv", "tasks/habits.csv"] {
        std::fs::write(root.join(path), HEADER).unwrap();
    }
    std::fs::write(root.join("tasks/SCHEMA.json"), SCHEMA).unwrap();
    let baseline_path = workspace.paths().sync_dir().join("baselines/tasks.csv");
    std::fs::create_dir_all(baseline_path.parent().unwrap()).unwrap();
    std::fs::write(baseline_path, baseline).unwrap();
    let metadata_paths = [
        "projects/work__sample/.METADATA.json",
        "archive/projects/work__sample/.METADATA.json",
    ];
    for relative in metadata_paths {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            path,
            r#"{"name":"work__sample","title":"Preserved","tasks":[]}"#,
        )
        .unwrap();
    }
    let remote = RefCell::new(BTreeMap::from([
        ("tasks/tasks.csv".to_owned(), remote_row),
        ("tasks/habits.csv".to_owned(), HEADER.to_owned()),
        ("tasks/SCHEMA.json".to_owned(), SCHEMA.to_owned()),
    ]));
    let result = sync_csvs_with_transport(
        &workspace,
        Direction::Both,
        |relative| remote.borrow().get(relative).cloned(),
        |relative, text| {
            remote
                .borrow_mut()
                .insert(relative.to_owned(), text.to_owned());
            true
        },
    )
    .unwrap();
    let tasks = std::fs::read_to_string(root.join("tasks/tasks.csv")).unwrap();
    assert_eq!(tasks, remote.borrow()["tasks/tasks.csv"]);
    assert_eq!(tasks.contains("Edited"), edited);
    assert_eq!(tasks.contains("T1"), edited);
    assert_eq!(result.outcomes[0].deleted, usize::from(!edited));
    assert_eq!(result.outcomes[0].soft_conflicts, usize::from(edited));
    for relative in metadata_paths {
        let local: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join(relative)).unwrap()).unwrap();
        let published: serde_json::Value =
            serde_json::from_str(&remote.borrow()[relative]).unwrap();
        assert_eq!(local, published);
        assert_eq!(local["title"], "Preserved");
        assert_eq!(
            local["tasks"],
            if edited {
                serde_json::json!(["T1"])
            } else {
                serde_json::json!([])
            }
        );
    }
}
