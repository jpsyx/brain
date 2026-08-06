use super::*;
use brain::tasks::schema::{LegacySemanticSync, TaskSchemaMigration, migrate_inactive};

#[test]
fn legacy_sync_migrates_then_syncs_and_a_second_legacy_machine_joins() {
    if !rclone_available() {
        eprintln!("skipping: rclone not on PATH");
        return;
    }
    let temporary = tempfile::tempdir().unwrap();
    let remote = temporary.path().join("remote");
    let first = temporary.path().join("first");
    let second = temporary.path().join("second");
    for root in [&remote, &first, &second] {
        std::fs::create_dir_all(root).unwrap();
    }
    write_legacy_state(&first);
    write_legacy_state(&second);
    let manifest = brain::workspace::WorkspaceManifest::new(workspace_id());
    manifest.write_new(&first).unwrap();
    std::fs::create_dir_all(remote.join(".config")).unwrap();
    std::fs::copy(
        brain::workspace::WorkspaceManifest::path(&first),
        brain::workspace::WorkspaceManifest::path(&remote),
    )
    .unwrap();
    std::fs::create_dir_all(second.join(".config")).unwrap();
    std::fs::copy(
        brain::workspace::WorkspaceManifest::path(&first),
        brain::workspace::WorkspaceManifest::path(&second),
    )
    .unwrap();
    std::fs::write(first.join("note.md"), b"portable").unwrap();
    let first_paths = workspace_paths(&temporary.path().join("first-home"), workspace_id());
    let second_paths = workspace_paths(&temporary.path().join("second-home"), workspace_id());

    let initial = run_for_workspace(
        &first,
        &remote,
        Direction::Resync,
        &first_paths,
        workspace_id(),
    );
    assert!(initial.exit_ok, "legacy resync failed: {initial:?}");
    semantic_sync(&first_paths, &first, &remote);
    migrate(
        &first_paths,
        &first,
        &temporary.path().join("first-backups"),
    );
    publish_transition(&first_paths, &first, &remote);
    semantic_sync(&first_paths, &first, &remote);
    let immediate = run_for_workspace(
        &first,
        &remote,
        Direction::Both,
        &first_paths,
        workspace_id(),
    );
    assert!(
        immediate.exit_ok,
        "immediate current sync failed: {immediate:?}"
    );

    let join = run_for_workspace(
        &second,
        &remote,
        Direction::Resync,
        &second_paths,
        workspace_id(),
    );
    assert!(
        join.exit_ok,
        "second-machine legacy resync failed: {join:?}"
    );
    assert_eq!(
        std::fs::read_to_string(second.join("tasks/SCHEMA.json")).unwrap(),
        "{}\n",
        "generic sync must not activate task UUID identity before migration"
    );
    migrate(
        &second_paths,
        &second,
        &temporary.path().join("second-backups"),
    );
    semantic_sync(&second_paths, &second, &remote);

    for relative in ["tasks/tasks.csv", "tasks/habits.csv", "tasks/SCHEMA.json"] {
        assert_eq!(
            std::fs::read(first.join(relative)).unwrap(),
            std::fs::read(second.join(relative)).unwrap(),
            "second machine did not converge for {relative}"
        );
        assert_eq!(
            std::fs::read(second.join(relative)).unwrap(),
            std::fs::read(remote.join(relative)).unwrap(),
            "remote did not converge for {relative}"
        );
    }
}

#[test]
fn current_unconfigured_workspace_setup_transitions_an_empty_remote_for_a_second_current_machine() {
    if !rclone_available() {
        eprintln!("skipping: rclone not on PATH");
        return;
    }
    let temporary = tempfile::tempdir().unwrap();
    let remote = temporary.path().join("remote");
    let first = temporary.path().join("first");
    let second = temporary.path().join("second");
    for root in [&remote, &first, &second] {
        std::fs::create_dir_all(root).unwrap();
    }
    write_legacy_state(&first);
    write_legacy_state(&second);
    let first_paths = workspace_paths(&temporary.path().join("first-home"), workspace_id());
    let second_paths = workspace_paths(&temporary.path().join("second-home"), workspace_id());
    migrate_unconfigured(
        &first_paths,
        &first,
        &temporary.path().join("first-backups"),
    );

    let transitioned = brain::sync::setup::prepare_current_schema_for_setup_with_transport(
        &first_paths,
        &first,
        None,
        false,
        |relative, _bytes| rclone_copy(&first.join(relative), &remote.join(relative)),
    )
    .unwrap();

    assert!(transitioned);
    semantic_sync(&first_paths, &first, &remote);
    migrate_unconfigured(
        &second_paths,
        &second,
        &temporary.path().join("second-backups"),
    );
    semantic_sync(&second_paths, &second, &remote);
    for relative in ["tasks/tasks.csv", "tasks/habits.csv", "tasks/SCHEMA.json"] {
        assert_eq!(
            std::fs::read(first.join(relative)).unwrap(),
            std::fs::read(second.join(relative)).unwrap(),
            "second current machine did not converge for {relative}"
        );
        assert_eq!(
            std::fs::read(second.join(relative)).unwrap(),
            std::fs::read(remote.join(relative)).unwrap(),
            "remote did not converge for {relative}"
        );
    }
}

fn write_legacy_state(root: &Path) {
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    std::fs::write(
        root.join("tasks/tasks.csv"),
        "task_id,task_name,assigned_to\nT1,Plan,pablo\n",
    )
    .unwrap();
    std::fs::write(
        root.join("tasks/habits.csv"),
        "task_id,task_name,assigned_to\nH1,Walk,pablo\n",
    )
    .unwrap();
    std::fs::write(root.join("tasks/SCHEMA.json"), "{}\n").unwrap();
}

fn semantic_sync(paths: &brain::workspace::WorkspacePaths, root: &Path, remote: &Path) {
    brain::sync::csv_sync::sync_csvs_with_transport(
        paths,
        root,
        Direction::Both,
        |relative| rclone_cat(&remote.join(relative)),
        |relative, text| rclone_copy_text(text, &remote.join(relative)),
    )
    .unwrap();
}

fn migrate(paths: &brain::workspace::WorkspacePaths, root: &Path, backup_base: &Path) {
    std::fs::create_dir_all(backup_base).unwrap();
    let backup = backup_base.join("rollout");
    migrate_inactive(TaskSchemaMigration {
        workspace_id: workspace_id(),
        workspace_root: root,
        task_store_lock: &paths.task_store_lock(),
        preexisting_backup_base: backup_base,
        backup_dir: &backup,
        legacy_semantic_sync: LegacySemanticSync::Complete,
    })
    .unwrap();
}

fn migrate_unconfigured(paths: &brain::workspace::WorkspacePaths, root: &Path, backup_base: &Path) {
    std::fs::create_dir_all(backup_base).unwrap();
    let backup = backup_base.join("rollout");
    migrate_inactive(TaskSchemaMigration {
        workspace_id: workspace_id(),
        workspace_root: root,
        task_store_lock: &paths.task_store_lock(),
        preexisting_backup_base: backup_base,
        backup_dir: &backup,
        legacy_semantic_sync: LegacySemanticSync::NotConfigured,
    })
    .unwrap();
}

fn publish_transition(paths: &brain::workspace::WorkspacePaths, root: &Path, remote: &Path) {
    let remote_schema = std::fs::read_to_string(remote.join("tasks/SCHEMA.json")).ok();
    brain::migration::publish_task_schema_transition_with_transport(
        paths,
        root,
        remote_schema.as_deref(),
        |relative, _bytes| rclone_copy(&root.join(relative), &remote.join(relative)),
    )
    .unwrap();
}

fn rclone_cat(path: &Path) -> Option<String> {
    let output = Command::new("rclone")
        .args(["cat", &path.to_string_lossy()])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8(output.stdout).ok())
        .flatten()
}

fn rclone_copy_text(text: &str, destination: &Path) -> bool {
    let temporary = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(temporary.path(), text).unwrap();
    rclone_copy(temporary.path(), destination)
}

fn rclone_copy(source: &Path, destination: &Path) -> bool {
    Command::new("rclone")
        .args([
            "copyto",
            &source.to_string_lossy(),
            &destination.to_string_lossy(),
        ])
        .status()
        .is_ok_and(|status| status.success())
}
