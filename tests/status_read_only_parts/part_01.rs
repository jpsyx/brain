#[test]
fn server_status_is_a_literal_read_only_process_probe() {
    let home = tempfile::tempdir().expect("temporary home");
    let before = snapshot(home.path());
    let before_logs = run_log_snapshot();

    let (pid, output) = run(home.path(), &["server", "status"]);

    assert!(output.status.success(), "{output:?}");
    assert_eq!(snapshot(home.path()), before);
    assert_eq!(
        pid_run_logs(pid, &run_log_snapshot()),
        pid_run_logs(pid, &before_logs),
        "status created or modified a PID run log"
    );
    let server = home.path().join(".cache/brain/server");
    assert!(!server.exists(), "status created server state");
}

#[test]
fn receiver_status_reads_four_fields_without_mutating_workspace_or_machine_state() {
    let home = tempfile::tempdir().expect("temporary home");
    seed_ready_workspace(home.path());
    let before = snapshot(home.path());
    let before_logs = run_log_snapshot();

    let (pid, output) = run(home.path(), &["-b", "brain", "receiver", "status"]);

    assert!(output.status.success(), "{output:?}");
    assert_eq!(snapshot(home.path()), before);
    assert_eq!(
        pid_run_logs(pid, &run_log_snapshot()),
        pid_run_logs(pid, &before_logs),
        "status created or modified a PID run log"
    );
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
    for line in [
        "Receiver  enabled",
        "TUI       not live",
        "Server    not running",
        "Accepting no",
        "Workspace brain",
        "receiver: incomplete",
        "SMS: off",
        "email: off",
    ] {
        assert!(stdout.contains(line), "missing `{line}` in:\n{stdout}");
    }
    assert!(!home.path().join(".cache/brain/server").exists());
}

#[test]
fn the_receiver_details_listing_and_both_addresses_write_nothing() {
    let home = tempfile::tempdir().expect("temporary home");
    seed_ready_workspace(home.path());
    set_workspace_env(home.path(), "resend_from_email", "brain@example.test");
    set_workspace_env(home.path(), "twilio_from_number", "+12125550100");
    let before = snapshot(home.path());
    let before_logs = run_log_snapshot();

    for (arguments, expected) in [
        (vec!["-b", "brain", "receiver"], "Receiver details  brain"),
        (
            vec!["-b", "brain", "receiver", "email"],
            "brain@example.test",
        ),
        (vec!["-b", "brain", "receiver", "phone"], "+12125550100"),
    ] {
        let (pid, output) = run(home.path(), &arguments);

        assert!(output.status.success(), "{arguments:?}: {output:?}");
        assert_eq!(snapshot(home.path()), before, "{arguments:?} wrote state");
        assert_eq!(
            pid_run_logs(pid, &run_log_snapshot()),
            pid_run_logs(pid, &before_logs),
            "{arguments:?} created or modified a PID run log"
        );
        let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
        assert!(stdout.contains(expected), "{arguments:?}:\n{stdout}");
        assert!(!home.path().join(".cache/brain/server").exists());
    }
}

fn set_workspace_env(home: &Path, name: &str, value: &str) {
    let registry_path = home.join(".config/brain/env.json");
    let mut registry: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&registry_path).expect("registry bytes"))
            .expect("registry JSON");
    registry["workspaces"]["brain"]["env"][name] = serde_json::json!(value);
    std::fs::write(
        &registry_path,
        serde_json::to_vec_pretty(&registry).expect("registry JSON"),
    )
    .expect("seed workspace env");
}

#[test]
fn sync_status_reports_partial_configuration_without_writing_any_state() {
    let home = tempfile::tempdir().expect("temporary home");
    seed_ready_workspace(home.path());
    let registry_path = home.path().join(".config/brain/env.json");
    let mut registry: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&registry_path).expect("registry bytes"))
            .expect("registry JSON");
    registry["workspaces"]["brain"]["env"]["sync"] =
        serde_json::json!({"enabled": true, "b2_bucket": "private-bucket"});
    std::fs::write(
        &registry_path,
        serde_json::to_vec_pretty(&registry).expect("registry JSON"),
    )
    .expect("partial sync config");
    let before = snapshot(home.path());
    let before_logs = run_log_snapshot();

    let (pid, output) = run(home.path(), &["-b", "brain", "sync", "status"]);

    assert!(output.status.success(), "{output:?}");
    assert_eq!(snapshot(home.path()), before);
    assert_eq!(
        pid_run_logs(pid, &run_log_snapshot()),
        pid_run_logs(pid, &before_logs),
        "sync status created or modified a PID run log"
    );
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
    assert!(stdout.contains("Workspace brain"), "{stdout}");
    assert!(stdout.contains("cloud sync: incomplete"), "{stdout}");
    assert!(!stdout.contains("private-bucket"), "{stdout}");
    assert!(!home.path().join(".cache/brain").exists());
    assert!(!home.path().join("brain/.config/config.json").exists());
}

#[test]
fn sync_status_reads_an_existing_wal_journal_without_mutating_it() {
    let home = tempfile::tempdir().expect("temporary home");
    seed_ready_workspace(home.path());
    let registry_path = home.path().join(".config/brain/env.json");
    let mut registry: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&registry_path).expect("registry bytes"))
            .expect("registry JSON");
    registry["workspaces"]["brain"]["env"]["sync"] = serde_json::json!({
        "enabled": true,
        "b2_bucket": "private-bucket",
        "b2_path": "",
        "b2_key_id": "private-key-id",
        "b2_app_key": "private-app-key",
        "watch": false
    });
    std::fs::write(
        &registry_path,
        serde_json::to_vec_pretty(&registry).expect("registry JSON"),
    )
    .expect("ready sync config");
    let workspace_id = brain::workspace::WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b")
        .expect("workspace UUID");
    let paths = brain::workspace::WorkspacePaths::new(home.path(), workspace_id);
    let journal =
        brain::sync::journal::Journal::open(&paths.sync_journal()).expect("create sync journal");
    journal
        .record(&brain::sync::journal::SyncRun {
            started_at: "2026-08-05T12:00:00Z".to_owned(),
            finished_at: "2026-08-05T12:00:01Z".to_owned(),
            direction: "both".to_owned(),
            outcome: "clean".to_owned(),
            transferred: 1,
            deleted: 0,
            conflicts: 0,
            errors: 0,
            note: String::new(),
        })
        .expect("record sync run");
    drop(journal);
    let before = snapshot(home.path());

    let (_, output) = run(home.path(), &["-b", "brain", "sync", "status"]);

    assert!(output.status.success(), "{output:?}");
    assert_eq!(snapshot(home.path()), before);
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
    assert!(stdout.contains("last sync:"), "{stdout}");
    assert!(!stdout.contains("private-bucket"), "{stdout}");
    assert!(!stdout.contains("private-key-id"), "{stdout}");
    assert!(!stdout.contains("private-app-key"), "{stdout}");
}
