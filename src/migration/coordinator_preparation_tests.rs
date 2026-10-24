//! Offline coordinator preparation must not cache a half-installed generation.
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use super::*;
use crate::tasks::store_lock::TaskStoreOwner;
use crate::tasks::store_transaction::{
    FileChange, TargetPolicy, TransactionStep, replace_group_with_hook,
};
use crate::workspace::{RegistryStore, WorkspaceContext, WorkspaceId, WorkspaceName};

fn fixture(assignment: &str) -> (tempfile::TempDir, CommandContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("workspace");
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    std::fs::create_dir_all(root.join(".config")).unwrap();
    std::fs::write(
        root.join("tasks/tasks.csv"),
        format!("task_id,task_name,assignee\nT1,Sample,{assignment}\n"),
    )
    .unwrap();
    std::fs::write(
        root.join("tasks/habits.csv"),
        "task_id,task_name,assignee\nH1,Walk,member\n",
    )
    .unwrap();
    std::fs::write(root.join("tasks/SCHEMA.json"), "{}\n").unwrap();
    std::fs::write(
        root.join(".config/config.json"),
        "{\"enable_triage_habits\":false}\n",
    )
    .unwrap();
    std::fs::write(
        root.join(".config/users.json"),
        r#"{"schema_version":1,"users":[{"id":"member","name":"Member","emails":[{"value":"member@example.test","inbound_allowed":true}],"response_email":"member@example.test"}]}"#,
    )
    .unwrap();
    let workspace = WorkspaceContext::new(
        temp.path(),
        WorkspaceId::new(),
        WorkspaceName::parse("sample").unwrap(),
        &root,
        "member",
        temp.path(),
    )
    .unwrap();
    let context = CommandContext::for_test(
        Arc::new(workspace),
        RegistryStore::from_path(temp.path().join("registry.json")),
        "member",
    );
    (temp, context)
}

#[test]
fn offline_coordinator_preparation_waits_then_validates_restored_assignments() {
    let (_temp, context) = fixture("legacy-person");
    let (paused_tx, paused_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let removal_workspace = context.workspace.clone();
    let removal = std::thread::spawn(move || {
        let owner = TaskStoreOwner::acquire(&removal_workspace).unwrap();
        let changes = ["tasks/tasks.csv", "tasks/habits.csv"].map(|relative| {
            let path = removal_workspace.root().join(relative);
            FileChange {
                before: Some(std::fs::read(&path).unwrap()),
                path,
                after: b"task_id,task_name,assignee\n".to_vec(),
            }
        });
        let crashed = catch_unwind(AssertUnwindSafe(|| {
            replace_group_with_hook(
                &removal_workspace,
                &owner,
                TargetPolicy::TaskRemoval,
                &changes,
                |step| {
                    if step == TransactionStep::Install(1) {
                        paused_tx.send(()).unwrap();
                        release_rx.recv().unwrap();
                        panic!("simulated removal crash");
                    }
                    Ok(())
                },
            )
            .unwrap();
        }));
        assert!(crashed.is_err());
    });
    paused_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let migration_context = context.clone();
    let migration = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        let result = super::prepare_migration(&migration_context, None);
        done_tx.send(()).unwrap();
        result.map(drop).map_err(|error| error.to_string())
    });
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let completed_early = done_rx.recv_timeout(Duration::from_millis(200)).is_ok();
    release_tx.send(()).unwrap();
    removal.join().unwrap();
    let result = migration.join().unwrap();
    assert!(
        !completed_early,
        "offline coordinator read assignments while removal was half-installed"
    );
    let error = result.unwrap_err();
    assert!(
        error.contains("legacy-person"),
        "restored assignment was omitted from preflight: {error}"
    );
    assert!(
        !context.workspace.paths().migration_journal().exists(),
        "mapping failures must still precede journal creation"
    );
}

#[test]
fn offline_coordinator_keeps_prepared_assignment_generation_owned_through_execution() {
    let (_temp, context) = fixture("member");
    let prepared = super::prepare_migration(&context, None).unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let workspace = context.workspace.clone();
    let writer = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        let _owner = TaskStoreOwner::acquire(&workspace).unwrap();
        done_tx.send(()).unwrap();
    });
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let completed_early = done_rx.recv_timeout(Duration::from_millis(200)).is_ok();
    let backup_base = context.workspace.paths().migration_backups();
    std::fs::create_dir_all(&backup_base).unwrap();
    let backup = backup_base.join("test");
    let mut journal = MigrationJournal::open_or_create(JournalRequest {
        path: &context.workspace.paths().migration_journal(),
        workspace_id: context.workspace.id(),
        workspace_root: context.workspace.root(),
        backup_dir: &backup,
        started_at: "2026-10-14T00:00:00Z",
        plan: &[Step::MigrateTaskColumnsAndUuids, Step::MarkComplete],
    })
    .unwrap();
    super::execute(
        &context,
        &crate::sync::config::SyncConfig::default(),
        None,
        Some(prepared),
        &backup_base,
        &mut journal,
    )
    .unwrap();
    writer.join().unwrap();
    assert!(
        !completed_early,
        "offline prepared assignments released ownership before execution"
    );
    let migrated =
        std::fs::read_to_string(context.workspace.root().join("tasks/tasks.csv")).unwrap();
    assert!(migrated.contains("task_uuid"), "{migrated}");
    assert!(migrated.contains("T1,Sample,member"), "{migrated}");
    assert!(!context.workspace.paths().migration_journal().exists());
}
