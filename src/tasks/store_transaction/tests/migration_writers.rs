use super::writers::writer_fixture;
use super::*;

#[test]
fn migration_owned_conversion_recovers_before_canonical_reads() {
    let (_temp, workspace, changes) = writer_fixture();
    std::fs::write(workspace.root().join("tasks/SCHEMA.json"), "{}\n").unwrap();
    let owner = TaskStoreOwner::acquire(&workspace).unwrap();
    crash_at(&workspace, &owner, &changes, TransactionStep::Install(1));
    let backup_base = workspace.paths().migration_backups();
    std::fs::create_dir_all(&backup_base).unwrap();
    let backup = backup_base.join("test");
    let lock = workspace.paths().task_store_lock();
    let rewrites = crate::users::AssignmentRewrites::new();
    let result = crate::tasks::schema::migrate_inactive_owned(
        &workspace,
        &owner,
        crate::tasks::schema::TaskSchemaMigration {
            workspace_id: workspace.id(),
            workspace_root: workspace.root(),
            task_store_lock: &lock,
            preexisting_backup_base: &backup_base,
            backup_dir: &backup,
            legacy_semantic_sync: crate::tasks::schema::LegacySemanticSync::NotConfigured,
            assignment_rewrites: &rewrites,
        },
    )
    .unwrap();
    assert_eq!(result, crate::tasks::schema::MigrationOutcome::Migrated);
    let text = std::fs::read_to_string(workspace.root().join("tasks/tasks.csv")).unwrap();
    assert!(
        text.contains("T1"),
        "migration read the half-installed deletion: {text}"
    );
    assert!(text.contains("task_uuid"));
}
