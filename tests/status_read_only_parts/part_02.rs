
#[test]
fn workspace_list_includes_selected_requirements_without_writing_state() {
    let home = tempfile::tempdir().expect("temporary home");
    seed_ready_workspace(home.path());
    let before = snapshot(home.path());
    let before_logs = run_log_snapshot();

    let (pid, output) = run(home.path(), &["-b", "brain", "workspace", "list"]);

    assert!(output.status.success(), "{output:?}");
    assert_eq!(snapshot(home.path()), before);
    assert_eq!(
        pid_run_logs(pid, &run_log_snapshot()),
        pid_run_logs(pid, &before_logs),
        "workspace list created or modified a PID run log"
    );
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
    assert!(stdout.contains("Workspaces"), "{stdout}");
    assert!(stdout.contains("Workspace brain"), "{stdout}");
    assert!(stdout.contains("workspace manifest: ready"), "{stdout}");
}

#[test]
fn tasks_doctor_is_grouped_by_workspace_and_does_not_write_state() {
    let home = tempfile::tempdir().expect("temporary home");
    seed_ready_workspace(home.path());
    let before = snapshot(home.path());
    let before_logs = run_log_snapshot();

    let (pid, output) = run(home.path(), &["-b", "brain", "tasks", "doctor"]);

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(snapshot(home.path()), before);
    assert_eq!(
        pid_run_logs(pid, &run_log_snapshot()),
        pid_run_logs(pid, &before_logs),
        "tasks doctor created or modified a PID run log"
    );
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
    assert!(stdout.contains("Workspace brain"), "{stdout}");
    assert!(stdout.contains("Claude SessionStart"), "{stdout}");
    assert!(stdout.contains("Codex SessionStart"), "{stdout}");
    assert!(!stdout.contains("OpenCode"), "{stdout}");
    assert!(stdout.contains("Features"), "{stdout}");
}

#[test]
fn tasks_doctor_reads_an_existing_wal_state_database_without_mutating_it() {
    let home = tempfile::tempdir().expect("temporary home");
    seed_ready_workspace(home.path());
    let workspace_id = brain::workspace::WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b")
        .expect("workspace UUID");
    let paths = brain::workspace::WorkspacePaths::new(home.path(), workspace_id);
    let state_db = paths.state_db();
    std::fs::create_dir_all(state_db.parent().expect("state parent")).expect("workspace cache");
    drop(brain::state::Db::open_path(&state_db).expect("create state database"));
    let before = snapshot(home.path());

    let (_, output) = run(home.path(), &["-b", "brain", "tasks", "doctor"]);

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(snapshot(home.path()), before);
}

#[test]
fn receiver_status_surfaces_a_live_process_control_failure() {
    let home = tempfile::tempdir().expect("temporary home");
    seed_ready_workspace(home.path());
    publish_live_process_record(home.path(), "57b162df-983a-45c3-ac7e-bad94eb27a99");

    let (_, output) = run(home.path(), &["-b", "brain", "receiver", "status"]);

    assert!(!output.status.success(), "{output:?}");
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 stderr");
    assert!(
        stderr.contains("connecting to the shared brain server"),
        "missing preserved control error:\n{stderr}"
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("TUI       not live"));
}
