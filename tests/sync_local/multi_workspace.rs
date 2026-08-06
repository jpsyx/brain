use super::*;

fn family_workspace_id() -> brain::workspace::WorkspaceId {
    brain::workspace::WorkspaceId::parse("e806258e-491a-436d-9db4-a5ca9903e0d4")
        .expect("valid family workspace id")
}

#[test]
fn concurrent_local_remotes_use_distinct_workspace_runtime_paths() {
    if !rclone_available() {
        eprintln!("skipping: rclone not on PATH");
        return;
    }
    let temporary = tempfile::tempdir().unwrap();
    let base = temporary.path();
    let personal_local = base.join("personal-local");
    let personal_remote = base.join("personal-remote");
    let family_local = base.join("family-local");
    let family_remote = base.join("family-remote");
    for path in [
        &personal_local,
        &personal_remote,
        &family_local,
        &family_remote,
    ] {
        std::fs::create_dir_all(path).unwrap();
    }
    std::fs::write(personal_local.join("personal.md"), b"personal").unwrap();
    std::fs::write(family_local.join("family.md"), b"family").unwrap();
    let personal_paths = workspace_paths(base, workspace_id());
    let family_paths = workspace_paths(base, family_workspace_id());

    let (personal, family) = std::thread::scope(|scope| {
        let personal_local = &personal_local;
        let personal_remote = &personal_remote;
        let personal_paths = &personal_paths;
        let personal = scope.spawn(move || {
            run_for_workspace(
                personal_local,
                personal_remote,
                Direction::Resync,
                personal_paths,
                workspace_id(),
            )
        });
        let family_local = &family_local;
        let family_remote = &family_remote;
        let family_paths = &family_paths;
        let family = scope.spawn(move || {
            run_for_workspace(
                family_local,
                family_remote,
                Direction::Resync,
                family_paths,
                family_workspace_id(),
            )
        });
        (personal.join().unwrap(), family.join().unwrap())
    });

    assert!(personal.exit_ok, "personal resync failed: {personal:?}");
    assert!(family.exit_ok, "family resync failed: {family:?}");
    assert_ne!(
        brain::sync::run::bisync_workdir(&personal_paths),
        brain::sync::run::bisync_workdir(&family_paths)
    );
    assert!(brain::sync::run::bisync_workdir(&personal_paths).exists());
    assert!(brain::sync::run::bisync_workdir(&family_paths).exists());

    let personal_tasks = personal_local.join("tasks/tasks.csv");
    let family_tasks = family_local.join("tasks/tasks.csv");
    std::fs::create_dir_all(personal_tasks.parent().unwrap()).unwrap();
    std::fs::create_dir_all(family_tasks.parent().unwrap()).unwrap();
    std::fs::write(
        &personal_tasks,
        "task_id,status,notes,last_touched\nT1,open,personal,t1\n",
    )
    .unwrap();
    std::fs::write(
        &family_tasks,
        "task_id,status,notes,last_touched\nT1,open,family,t1\n",
    )
    .unwrap();
    std::thread::scope(|scope| {
        let personal = scope.spawn(|| {
            brain::sync::csv_sync::sync_one(
                &personal_paths,
                &personal_tasks,
                "tasks/tasks.csv",
                || std::fs::read_to_string(&personal_tasks).ok(),
                |_| true,
            )
        });
        let family = scope.spawn(|| {
            brain::sync::csv_sync::sync_one(
                &family_paths,
                &family_tasks,
                "tasks/tasks.csv",
                || std::fs::read_to_string(&family_tasks).ok(),
                |_| true,
            )
        });
        assert_eq!(personal.join().unwrap().name, "tasks.csv");
        assert_eq!(family.join().unwrap().name, "tasks.csv");
    });
    let personal_baseline = brain::sync::csv_sync::baseline_path(&personal_paths, "tasks.csv");
    let family_baseline = brain::sync::csv_sync::baseline_path(&family_paths, "tasks.csv");
    assert_ne!(personal_baseline, family_baseline);
    assert!(personal_baseline.exists());
    assert!(family_baseline.exists());

    let mismatch_home = base.join("mismatch-home");
    let mismatch_local = base.join("mismatch-local");
    let mismatch_remote = base.join("mismatch-remote");
    brain::workspace::WorkspaceManifest::new(workspace_id())
        .write_new(&mismatch_local)
        .unwrap();
    brain::workspace::WorkspaceManifest::new(family_workspace_id())
        .write_new(&mismatch_remote)
        .unwrap();
    std::fs::write(mismatch_local.join("must-not-sync.md"), b"local only").unwrap();
    let mismatch_paths = workspace_paths(&mismatch_home, workspace_id());
    let mismatch_target = Remote {
        env: Vec::new(),
        arg: mismatch_remote.to_string_lossy().into_owned(),
    };

    let error = brain::sync::identity::require_remote_identity(
        &mismatch_local,
        workspace_id(),
        &mismatch_target,
    )
    .expect_err("remote UUID mismatch must refuse before bisync");
    let message = error.to_string();
    assert!(
        message.contains(&family_workspace_id().to_string())
            && message.contains(&workspace_id().to_string()),
        "{error:#}"
    );
    assert!(!brain::sync::run::bisync_workdir(&mismatch_paths).exists());
    assert!(!mismatch_remote.join("must-not-sync.md").exists());
}
