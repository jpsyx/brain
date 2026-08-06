use std::path::Path;

use brain::migration::{
    JournalRequest, MigrationJournal, MigrationState, PlanInput, Step, migration_plan,
};
use brain::workspace::{WorkspaceId, WorkspacePaths};

const WORKSPACE_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";

#[test]
fn configured_legacy_plan_finishes_legacy_sync_before_uuid_cutover() {
    let plan = migration_plan(PlanInput {
        state: MigrationState::Legacy,
        sync_configured: true,
    })
    .expect("legacy workspace has a rollout plan");

    assert_eq!(
        plan,
        [
            Step::LegacySemanticSync,
            Step::BackupPortableData,
            Step::EnsureWorkspaceManifest,
            Step::EnsureUsersRegistry,
            Step::MigrateTaskColumnsAndUuids,
            Step::ReconcileManagedTriage,
            Step::RebuildDerivedData,
            Step::Verify,
            Step::MarkComplete,
        ]
    );
}

#[test]
fn unconfigured_legacy_plan_omits_the_legacy_sync_step() {
    let plan = migration_plan(PlanInput {
        state: MigrationState::Legacy,
        sync_configured: false,
    })
    .expect("local-only legacy workspace has a rollout plan");

    assert_eq!(
        plan,
        [
            Step::BackupPortableData,
            Step::EnsureWorkspaceManifest,
            Step::EnsureUsersRegistry,
            Step::MigrateTaskColumnsAndUuids,
            Step::ReconcileManagedTriage,
            Step::RebuildDerivedData,
            Step::Verify,
            Step::MarkComplete,
        ]
    );
}

#[test]
fn prepared_workspace_still_runs_cutover_steps() {
    let plan = migration_plan(PlanInput {
        state: MigrationState::Prepared,
        sync_configured: false,
    })
    .expect("prepared legacy workspace still needs task cutover");

    assert_eq!(plan.first(), Some(&Step::BackupPortableData));
    assert!(plan.contains(&Step::EnsureWorkspaceManifest));
    assert!(plan.contains(&Step::EnsureUsersRegistry));
    assert!(plan.contains(&Step::MigrateTaskColumnsAndUuids));
    assert_eq!(plan.last(), Some(&Step::MarkComplete));
}

#[test]
fn current_workspace_has_no_rollout_steps() {
    assert_eq!(
        migration_plan(PlanInput {
            state: MigrationState::Current,
            sync_configured: true,
        })
        .expect("current workspace is an idempotent no-op"),
        []
    );
}

#[test]
fn newer_workspace_is_refused_without_a_plan() {
    let error = migration_plan(PlanInput {
        state: MigrationState::NewerRefused { found: 3 },
        sync_configured: false,
    })
    .expect_err("newer task schema must fail closed");

    assert!(error.to_string().contains("task schema 3"), "{error:#}");
    assert!(error.to_string().contains("supports schema 2"), "{error:#}");
}

#[test]
fn migration_runtime_paths_are_scoped_to_the_selected_workspace_uuid() {
    let paths = WorkspacePaths::new(
        Path::new("/home/tester"),
        WorkspaceId::parse(WORKSPACE_ID).unwrap(),
    );
    let cache = Path::new("/home/tester/.cache/brain/workspaces").join(WORKSPACE_ID);

    assert_eq!(
        paths.migration_journal(),
        cache.join("migrations/multi-workspace-v1.json")
    );
    assert_eq!(paths.migration_backups(), cache.join("migration-backups"));
}

#[test]
fn interrupted_journal_resumes_after_the_last_verified_step() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("workspace");
    let path = temporary
        .path()
        .join("cache/migrations/multi-workspace-v1.json");
    let backup = temporary
        .path()
        .join("cache/migration-backups/20260806T120000Z-pre-multi-workspace");
    std::fs::create_dir(&root).unwrap();
    let workspace_id = WorkspaceId::parse(WORKSPACE_ID).unwrap();
    let plan = migration_plan(PlanInput {
        state: MigrationState::Legacy,
        sync_configured: true,
    })
    .unwrap();
    let request = JournalRequest {
        path: &path,
        workspace_id,
        workspace_root: &root,
        backup_dir: &backup,
        started_at: "2026-08-06T12:00:00Z",
        plan: &plan,
    };
    let mut journal = MigrationJournal::open_or_create(request).unwrap();
    journal.record_completed(Step::LegacySemanticSync).unwrap();
    journal.record_completed(Step::BackupPortableData).unwrap();

    let resumed = MigrationJournal::open_or_create(request).unwrap();

    assert_eq!(resumed.backup_dir(), backup);
    assert_eq!(
        resumed.remaining_steps(),
        &plan[2..],
        "completed steps must not replay after interruption"
    );
    assert!(path.is_file());
}

#[test]
fn interrupted_journal_keeps_original_backup_when_reentered_later() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("workspace");
    let path = temporary
        .path()
        .join("cache/migrations/multi-workspace-v1.json");
    let backup = temporary.path().join("cache/migration-backups/original");
    std::fs::create_dir(&root).unwrap();
    let workspace_id = WorkspaceId::parse(WORKSPACE_ID).unwrap();
    let plan = migration_plan(PlanInput {
        state: MigrationState::Legacy,
        sync_configured: false,
    })
    .unwrap();
    MigrationJournal::open_or_create(JournalRequest {
        path: &path,
        workspace_id,
        workspace_root: &root,
        backup_dir: &backup,
        started_at: "2026-08-06T12:00:00Z",
        plan: &plan,
    })
    .unwrap();

    let resumed = MigrationJournal::resume(&path, workspace_id, &root, &plan).unwrap();

    assert_eq!(resumed.backup_dir(), backup);
}

#[test]
fn completed_journal_is_removed_while_its_backup_is_retained() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("workspace");
    let path = temporary
        .path()
        .join("cache/migrations/multi-workspace-v1.json");
    let backup = temporary.path().join("cache/migration-backups/rollout");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir_all(&backup).unwrap();
    std::fs::write(backup.join("recovery.txt"), b"retained").unwrap();
    let workspace_id = WorkspaceId::parse(WORKSPACE_ID).unwrap();
    let plan = migration_plan(PlanInput {
        state: MigrationState::Legacy,
        sync_configured: false,
    })
    .unwrap();
    let mut journal = MigrationJournal::open_or_create(JournalRequest {
        path: &path,
        workspace_id,
        workspace_root: &root,
        backup_dir: &backup,
        started_at: "2026-08-06T12:00:00Z",
        plan: &plan,
    })
    .unwrap();
    for step in &plan[..plan.len() - 1] {
        journal.record_completed(*step).unwrap();
    }

    journal.mark_complete().unwrap();

    assert!(!path.exists());
    assert_eq!(
        std::fs::read(backup.join("recovery.txt")).unwrap(),
        b"retained"
    );
}
