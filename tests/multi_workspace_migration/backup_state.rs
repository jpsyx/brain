use std::path::Path;

use brain::migration::{MigrationState, backup_directory, backup_portable_data, discover_state};

#[test]
fn backup_copies_only_the_exact_portable_migration_inventory() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("workspace");
    let backup_base = temporary.path().join("selected-cache/migration-backups");
    let expected = [
        (".config/config.json", b"{\"portable\":true}\n".as_slice()),
        (
            ".config/personalization.json",
            b"{\"name\":\"Alex\"}\n".as_slice(),
        ),
        (
            ".config/users.json",
            b"{\"schema_version\":1,\"users\":[]}\n".as_slice(),
        ),
        (
            ".config/workspace.json",
            b"{\"schema_version\":1,\"workspace_id\":\"8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b\"}\n"
                .as_slice(),
        ),
        (
            "tasks/tasks.csv",
            b"task_id,task_name\nT1,Plan\n".as_slice(),
        ),
        (
            "tasks/habits.csv",
            b"task_id,task_name\nH1,Walk\n".as_slice(),
        ),
        ("tasks/.tasks_next_id", b"2\n".as_slice()),
        ("tasks/.habits_next_id", b"2\n".as_slice()),
        ("tasks/SCHEMA.json", b"{\"label\":\"Tasks\"}\n".as_slice()),
    ];
    for (relative, bytes) in expected {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    for relative in [
        ".config/credentials.json",
        "messages/inbound.json",
        "cache/state.db",
        "jobs.sock",
    ] {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"must not be backed up").unwrap();
    }
    let backup = backup_directory(&backup_base, "20260806T120000Z").unwrap();

    backup_portable_data(&root, &backup_base, &backup).unwrap();

    assert_eq!(
        backup,
        backup_base.join("20260806T120000Z-pre-multi-workspace")
    );
    for (relative, bytes) in expected {
        assert_eq!(std::fs::read(backup.join(relative)).unwrap(), bytes);
    }
    for relative in [
        ".config/credentials.json",
        "messages/inbound.json",
        "cache/state.db",
        "jobs.sock",
    ] {
        assert!(
            !backup.join(relative).exists(),
            "excluded {relative} leaked"
        );
    }
}

#[test]
fn discovery_distinguishes_legacy_prepared_current_and_newer_workspaces() {
    let temporary = tempfile::tempdir().unwrap();
    let legacy = temporary.path().join("legacy");
    write_legacy_tasks(&legacy);
    assert_eq!(discover_state(&legacy).unwrap(), MigrationState::Legacy);

    let prepared = temporary.path().join("prepared");
    write_legacy_tasks(&prepared);
    std::fs::create_dir_all(prepared.join(".config")).unwrap();
    std::fs::write(prepared.join(".config/workspace.json"), b"prepared\n").unwrap();
    assert_eq!(discover_state(&prepared).unwrap(), MigrationState::Prepared);

    let current = temporary.path().join("current");
    write_current_tasks(&current, 2);
    assert_eq!(discover_state(&current).unwrap(), MigrationState::Current);

    let newer = temporary.path().join("newer");
    write_current_tasks(&newer, 3);
    assert_eq!(
        discover_state(&newer).unwrap(),
        MigrationState::NewerRefused { found: 3 }
    );
}

fn write_legacy_tasks(root: &Path) {
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    std::fs::write(
        root.join("tasks/tasks.csv"),
        b"task_id,task_name,assigned_to\nT1,Plan,alex\n",
    )
    .unwrap();
    std::fs::write(
        root.join("tasks/habits.csv"),
        b"task_id,task_name,assigned_to\nH1,Walk,alex\n",
    )
    .unwrap();
    std::fs::write(root.join("tasks/SCHEMA.json"), b"{}\n").unwrap();
}

fn write_current_tasks(root: &Path, version: u64) {
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    let header = b"task_uuid,task_id,task_name,assigned_to,system_key\n";
    std::fs::write(root.join("tasks/tasks.csv"), header).unwrap();
    std::fs::write(root.join("tasks/habits.csv"), header).unwrap();
    std::fs::write(
        root.join("tasks/SCHEMA.json"),
        format!(
            "{{\"task_schema_version\":{version},\"merge_key\":\"task_uuid\",\"display_identity\":{{\"field\":\"task_id\",\"mutable\":true}}}}\n"
        ),
    )
    .unwrap();
}
