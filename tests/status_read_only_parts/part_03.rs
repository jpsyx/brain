
#[test]
fn receiver_status_rejects_generation_replacement_in_its_single_control_probe() {
    let home = tempfile::tempdir().expect("temporary home");
    seed_ready_workspace(home.path());
    let paths = brain::server::lifecycle::ServerPaths::from_home(home.path());
    std::fs::create_dir_all(paths.directory()).expect("server state directory");
    let listener = UnixListener::bind(paths.control_socket()).expect("control listener");
    publish_live_process_record(home.path(), "57b162df-983a-45c3-ac7e-bad94eb27a99");
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept status probe");
        let request =
            brain::server::control::codec::read::<brain::server::control::ControlRequest>(
                &mut stream,
            )
            .expect("read status probe");
        assert!(matches!(
            request,
            brain::server::control::ControlRequest::WorkspaceStatus { .. }
        ));
        brain::server::control::codec::write(
            &mut stream,
            &brain::server::control::ControlResponse::StaleGeneration,
        )
        .expect("write stale generation");
    });

    let (_, output) = run(home.path(), &["-b", "brain", "receiver", "status"]);

    assert!(!output.status.success(), "{output:?}");
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 stderr");
    assert!(stderr.contains("generation changed"), "{stderr}");
    server.join().expect("status probe server");
}

#[test]
fn concurrent_status_commands_leave_an_active_generation_exactly_unchanged() {
    let mut fixture = DualWorkspaceReceiverFixture::start();
    seed_current_migration(fixture.home());
    let before_filesystem = snapshot(fixture.home());
    let before_server = fixture.server_snapshot();
    let control_socket = fixture.home().join(".cache/brain/server/control.sock");
    let before_control_socket = snapshot_entry(&control_socket);
    let before_logs = run_log_snapshot();

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(9));
    let workers = (0..8)
        .map(|index| {
            let home = fixture.home().to_path_buf();
            let barrier = std::sync::Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                if index % 2 == 0 {
                    run(&home, &["server", "status"])
                } else {
                    run(&home, &["-b", "personal", "receiver", "status"])
                }
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let mut pids = Vec::with_capacity(workers.len());
    for worker in workers {
        let (pid, output) = worker.join().expect("status worker");
        pids.push(pid);
        assert!(output.status.success(), "{output:?}");
    }
    let after_logs = run_log_snapshot();
    for pid in pids {
        assert!(
            pid_run_logs_unchanged(pid, &before_logs, &after_logs),
            "active status created or modified a PID run log for {pid}"
        );
    }

    let after_server = fixture.server_snapshot();
    assert_eq!(after_server, before_server);
    assert_eq!(snapshot(fixture.home()), before_filesystem);
    assert_eq!(snapshot_entry(&control_socket), before_control_socket);
    assert!(fixture.server_is_running());
    fixture.shutdown();
}

#[test]
fn receiver_status_is_read_only_through_symlinked_config_and_workspace_paths() {
    let home = tempfile::tempdir().expect("temporary home");
    let external = tempfile::tempdir().expect("external status state");
    seed_ready_workspace(home.path());
    let external_config = external.path().join("config");
    let external_brain = external.path().join("brain");
    std::fs::rename(home.path().join(".config"), &external_config).expect("move machine config");
    std::fs::rename(home.path().join("brain"), &external_brain).expect("move workspace");
    std::os::unix::fs::symlink(&external_config, home.path().join(".config"))
        .expect("link machine config");
    std::os::unix::fs::symlink(&external_brain, home.path().join("brain")).expect("link workspace");
    std::os::unix::fs::symlink(home.path(), external_brain.join("cycle"))
        .expect("link snapshot cycle");
    let before = snapshot(home.path());

    let (_, output) = run(home.path(), &["-b", "brain", "receiver", "status"]);

    assert!(output.status.success(), "{output:?}");
    assert_eq!(snapshot(home.path()), before);
}

fn run(home: &Path, arguments: &[&str]) -> (u32, Output) {
    let child = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(arguments)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .current_dir(home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn brain status");
    let pid = child.id();
    let output = child.wait_with_output().expect("wait for brain status");
    (pid, output)
}
