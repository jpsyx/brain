use brain::server::control::{HeartbeatEvent, HeartbeatWorker, LeaseRegistration};
use brain::server::lifecycle::{
    ElectionGuard, ProcessRecord, ServerClient, ServerDecision, ServerGeneration, ServerPaths,
    StartDecision, connect_or_elect, decide_start, validate_election_token,
};
use brain::server::lifecycle::{IngressId, LeaseId, WorkspaceLease};
use brain::workspace::{WorkspaceId, WorkspaceName};
use clap::Parser as _;
use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixListener;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn record() -> ProcessRecord {
    ProcessRecord {
        pid: 42,
        port: 8787,
        generation: ServerGeneration::parse("57b162df-983a-45c3-ac7e-bad94eb27a99")
            .expect("valid generation"),
        started_at: "2026-08-04T12:00:00Z".to_owned(),
    }
}

#[test]
fn a_live_record_is_reused_without_election() {
    let existing = record();

    assert_eq!(
        decide_start(Some(&existing), true, true, false),
        StartDecision::Reuse(existing)
    );
}

#[test]
fn an_elected_starter_cleans_stale_state_before_starting() {
    assert_eq!(
        decide_start(Some(&record()), false, false, true),
        StartDecision::Start {
            remove_stale_state: true
        }
    );
}

#[test]
fn exactly_one_contender_starts_when_no_record_exists() {
    assert_eq!(
        decide_start(None, false, false, true),
        StartDecision::Start {
            remove_stale_state: false
        }
    );
    assert_eq!(
        decide_start(None, false, false, false),
        StartDecision::WaitForWinner
    );
}

#[test]
fn a_losing_stale_record_contender_waits_for_the_winner() {
    assert_eq!(
        decide_start(Some(&record()), false, false, false),
        StartDecision::WaitForWinner
    );
}

#[test]
fn global_server_paths_share_one_owned_directory() {
    let paths = ServerPaths::from_home(std::path::Path::new("/home/tester"));

    assert_eq!(
        paths.directory(),
        std::path::Path::new("/home/tester/.cache/brain/server")
    );
    assert_eq!(
        paths.process_record(),
        paths.directory().join("process.json")
    );
    assert_eq!(
        paths.control_socket(),
        paths.directory().join("control.sock")
    );
    assert_eq!(
        paths.election_lock(),
        paths.directory().join("election.lock")
    );
    assert_eq!(paths.log(), paths.directory().join("server.log"));
}

#[test]
fn process_record_serialization_contains_only_infrastructure_state() {
    let value = serde_json::to_value(record()).expect("serialize process record");

    assert_eq!(
        value,
        serde_json::json!({
            "pid": 42,
            "port": 8787,
            "generation": "57b162df-983a-45c3-ac7e-bad94eb27a99",
            "started_at": "2026-08-04T12:00:00Z"
        })
    );
}

#[test]
fn election_lock_selects_one_starter_and_carries_the_hidden_run_token() {
    let temporary = tempfile::tempdir().expect("temporary server directory");
    let paths = ServerPaths::from_directory(temporary.path().join("server"));
    let winner =
        ServerGeneration::parse("57b162df-983a-45c3-ac7e-bad94eb27a99").expect("valid UUID");
    let loser =
        ServerGeneration::parse("91a0cfc2-7427-49d5-a2f1-258f985cd7e5").expect("valid UUID");

    let guard = ElectionGuard::try_acquire(&paths, winner)
        .expect("election probe")
        .expect("first contender wins");

    assert!(validate_election_token(&paths, winner).is_ok());
    assert!(validate_election_token(&paths, loser).is_err());
    assert!(
        ElectionGuard::try_acquire(&paths, loser)
            .expect("losing election probe")
            .is_none()
    );

    drop(guard);
    assert!(
        !paths.election_lock().exists(),
        "released winner token remained at {}",
        paths.election_lock().display()
    );
    assert!(
        ElectionGuard::try_acquire(&paths, loser)
            .expect("election after release")
            .is_some()
    );
}

#[test]
fn hidden_server_token_validation_refuses_an_unelected_generation() {
    let temporary = tempfile::tempdir().expect("temporary server directory");
    let paths = ServerPaths::from_directory(temporary.path().join("server"));
    let generation =
        ServerGeneration::parse("57b162df-983a-45c3-ac7e-bad94eb27a99").expect("valid UUID");

    let error = validate_election_token(&paths, generation).expect_err("missing election token");

    assert!(error.to_string().contains("election token"), "{error:#}");
}

#[test]
fn server_command_exposes_only_status_logs_and_the_token_guarded_hidden_run() {
    use brain::cli::{Cli, Cmd, ServerAction};

    assert!(matches!(
        Cli::try_parse_from(["brain", "server", "status"]),
        Ok(Cli {
            command: Some(Cmd::Server(args)),
            ..
        }) if matches!(args.action, ServerAction::Status)
    ));
    assert!(matches!(
        Cli::try_parse_from(["brain", "server", "logs"]),
        Ok(Cli {
            command: Some(Cmd::Server(args)),
            ..
        }) if matches!(args.action, ServerAction::Logs)
    ));
    assert!(Cli::try_parse_from(["brain", "server", "start"]).is_err());
    assert!(Cli::try_parse_from(["brain", "server", "kill"]).is_err());
    assert!(
        Cli::try_parse_from(["brain", "server", "run", "--port", "0"]).is_err(),
        "the hidden loop must require an election generation"
    );
    assert!(
        Cli::try_parse_from([
            "brain",
            "server",
            "run",
            "--generation",
            "57b162df-983a-45c3-ac7e-bad94eb27a99",
            "--port",
            "0"
        ])
        .is_ok()
    );
}

#[test]
fn connect_or_elect_reuses_an_existing_generation() {
    let mut server = RunningServer::start();

    let connected = connect_or_elect(&server.client).expect("reuse running server");

    assert_eq!(connected.generation, server.generation);
    server.shutdown_with_two_leases();
}

#[test]
fn final_unregister_stops_the_process_and_removes_generation_artifacts() {
    let mut server = RunningServer::start();
    let family = lease(
        "family",
        "e806258e-491a-436d-9db4-a5ca9903e0d4",
        "57b162df-983a-45c3-ac7e-bad94eb27a99",
        "00000000-0000-0000-0000-000000000001",
    );
    let personal = lease(
        "personal",
        "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b",
        "91a0cfc2-7427-49d5-a2f1-258f985cd7e5",
        "00000000-0000-0000-0000-000000000002",
    );
    server.client.register(&family).expect("register family");
    server
        .client
        .register(&personal)
        .expect("register personal");

    assert_eq!(
        server
            .client
            .unregister(family.lease_id)
            .expect("unregister family"),
        ServerDecision::KeepRunning
    );
    assert!(server.client.connect_existing().is_ok());
    assert_eq!(
        server
            .client
            .unregister(personal.lease_id)
            .expect("unregister personal"),
        ServerDecision::ShutdownNow
    );

    wait_for("shared server process exit", || {
        server.child.try_wait().ok().flatten().is_some()
    });
    wait_for("generation artifact cleanup", || {
        !server.paths.process_record().exists()
            && !server.paths.control_socket().exists()
            && !server.paths.election_lock().exists()
    });
}

#[test]
fn two_tui_heartbeats_race_recovery_and_share_one_replacement_generation() {
    let mut server = RunningServer::start();
    let family = lease(
        "family",
        "e806258e-491a-436d-9db4-a5ca9903e0d4",
        "57b162df-983a-45c3-ac7e-bad94eb27a99",
        "00000000-0000-0000-0000-000000000011",
    );
    let personal = lease(
        "personal",
        "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b",
        "91a0cfc2-7427-49d5-a2f1-258f985cd7e5",
        "00000000-0000-0000-0000-000000000012",
    );
    let family_registration = registration(server.generation, &family);
    let personal_registration = registration(server.generation, &personal);
    server
        .client
        .register_generation(&family_registration)
        .expect("register family");
    server
        .client
        .register_generation(&personal_registration)
        .expect("register personal");
    let mut family_worker = HeartbeatWorker::start(server.client.clone(), family_registration);
    let mut personal_worker = HeartbeatWorker::start(server.client.clone(), personal_registration);

    server.child.kill().expect("crash shared server");
    server.child.wait().expect("reap crashed server");
    let mut family_recovered = None;
    let mut personal_recovered = None;
    wait_for("both TUI leases to recover", || {
        for event in family_worker.poll() {
            if let HeartbeatEvent::Recovered(generation) = event {
                family_recovered = Some(generation);
            }
        }
        for event in personal_worker.poll() {
            if let HeartbeatEvent::Recovered(generation) = event {
                personal_recovered = Some(generation);
            }
        }
        family_recovered.is_some() && personal_recovered.is_some()
    });

    assert_eq!(family_recovered, personal_recovered);
    assert_ne!(family_recovered, Some(server.generation));
    family_worker.shutdown().expect("unregister family");
    personal_worker.shutdown().expect("unregister personal");
    wait_for("replacement generation cleanup", || {
        !server.paths.process_record().exists()
            && !server.paths.control_socket().exists()
            && !server.paths.election_lock().exists()
    });
}

#[test]
fn elected_process_stops_when_its_caller_never_registers() {
    let mut server = RunningServer::start();

    wait_for("unregistered elected process exit", || {
        server.child.try_wait().ok().flatten().is_some()
    });
    wait_for("unregistered generation artifact cleanup", || {
        !server.paths.process_record().exists()
            && !server.paths.control_socket().exists()
            && !server.paths.election_lock().exists()
    });
}

#[test]
fn termination_signal_runs_generation_guarded_cleanup() {
    let mut server = RunningServer::start();
    let personal = lease(
        "personal",
        "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b",
        "91a0cfc2-7427-49d5-a2f1-258f985cd7e5",
        "00000000-0000-0000-0000-000000000005",
    );
    server
        .client
        .register(&personal)
        .expect("register personal");

    let status = Command::new("kill")
        .args(["-TERM", &server.child.id().to_string()])
        .status()
        .expect("signal shared server");
    assert!(status.success());

    wait_for("signaled server cleanup", || {
        let exited = server.child.try_wait().ok().flatten().is_some();
        exited
            && !server.paths.process_record().exists()
            && !server.paths.control_socket().exists()
            && !server.paths.election_lock().exists()
    });
}

#[test]
fn signal_after_publication_in_the_startup_window_cleans_all_artifacts() {
    let home = tempfile::tempdir().expect("temporary server home");
    let paths = ServerPaths::from_home(home.path());
    let generation = ServerGeneration::new();
    let election = ElectionGuard::try_acquire(&paths, generation)
        .expect("election probe")
        .expect("test process wins election");
    let gate_path = home.path().join("startup-gate.sock");
    let gate = UnixListener::bind(&gate_path).expect("bind startup gate");
    gate.set_nonblocking(true)
        .expect("make startup gate nonblocking");
    let mut child = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args([
            "server",
            "run",
            "--generation",
            &generation.to_string(),
            "--port",
            "0",
        ])
        .env("HOME", home.path())
        .env("BRAIN_TEST_SERVER_STARTUP_GATE", &gate_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn gated hidden server");
    let handoff = election.handoff();
    let mut gated = None;
    wait_for("server startup gate", || match gate.accept() {
        Ok((stream, _)) => {
            gated = Some(stream);
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => false,
        Err(error) => panic!("accept startup gate: {error}"),
    });
    let mut gated = gated.expect("accepted startup gate");
    gated
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("bound startup gate read");
    let mut ready = [0; 5];
    gated.read_exact(&mut ready).expect("read startup ready");
    assert_eq!(&ready, b"ready");
    handoff.cleanup().expect("finish election handoff");
    assert!(paths.process_record().exists());
    assert!(paths.control_socket().exists());
    assert!(paths.election_lock().exists());
    let status = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .expect("signal gated shared server");
    assert!(status.success());
    let _ = gated.write_all(b"release");
    wait_for("gated server exit", || {
        child.try_wait().ok().flatten().is_some()
    });

    assert!(!paths.process_record().exists());
    assert!(!paths.control_socket().exists());
    assert!(!paths.election_lock().exists());
}

#[test]
fn bind_failure_before_publication_cleans_early_artifacts() {
    let home = tempfile::tempdir().expect("temporary server home");
    let paths = ServerPaths::from_home(home.path());
    let occupied = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("occupy loopback port");
    let port = occupied.local_addr().expect("occupied address").port();
    let generation = ServerGeneration::new();
    let election = ElectionGuard::try_acquire(&paths, generation)
        .expect("election probe")
        .expect("test process wins election");
    let mut child = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args([
            "server",
            "run",
            "--generation",
            &generation.to_string(),
            "--port",
            &port.to_string(),
        ])
        .env("HOME", home.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn hidden server with occupied port");
    let handoff = election.handoff();

    wait_for("failed server exit", || {
        child.try_wait().ok().flatten().is_some()
    });
    handoff.cleanup().expect("finish election handoff");

    assert!(!paths.process_record().exists());
    assert!(!paths.control_socket().exists());
    assert!(!paths.election_lock().exists());
}

#[test]
fn status_and_logs_are_read_only_without_a_workspace_or_server() {
    let home = tempfile::tempdir().expect("temporary home");
    let paths = ServerPaths::from_home(home.path());

    for action in ["status", "logs"] {
        let output = Command::new(env!("CARGO_BIN_EXE_brain"))
            .args(["server", action])
            .env("HOME", home.path())
            .stdin(Stdio::null())
            .output()
            .expect("run read-only server command");
        assert!(
            output.status.success(),
            "{action}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert!(!paths.directory().exists());
}

fn lease(name: &str, workspace_id: &str, ingress_id: &str, lease_id: &str) -> WorkspaceLease {
    WorkspaceLease {
        lease_id: LeaseId::parse(lease_id).expect("valid lease ID"),
        workspace_id: WorkspaceId::parse(workspace_id).expect("valid workspace ID"),
        canonical_name: WorkspaceName::parse(name).expect("valid workspace name"),
        ingress_id: IngressId::parse(ingress_id).expect("valid ingress ID"),
        tui_pid: std::process::id(),
        job_socket: std::path::PathBuf::from("/tmp/brain-job.sock"),
        receiver_enabled: true,
        expires_at: Instant::now() + Duration::from_secs(30),
    }
}

fn registration(generation: ServerGeneration, lease: &WorkspaceLease) -> LeaseRegistration {
    LeaseRegistration {
        generation,
        lease_id: lease.lease_id,
        workspace_id: lease.workspace_id,
        canonical_name: lease.canonical_name.to_string(),
        ingress_id: lease.ingress_id,
        tui_pid: lease.tui_pid,
        job_socket: lease.job_socket.clone(),
    }
}

struct RunningServer {
    child: Child,
    _home: tempfile::TempDir,
    paths: ServerPaths,
    client: ServerClient,
    generation: ServerGeneration,
}

impl RunningServer {
    fn start() -> Self {
        let home = tempfile::tempdir().expect("temporary server home");
        prepare_workspace_registry(home.path());
        let paths = ServerPaths::from_home(home.path());
        let generation = ServerGeneration::new();
        let election = ElectionGuard::try_acquire(&paths, generation)
            .expect("election probe")
            .expect("test process wins election");
        let child = Command::new(env!("CARGO_BIN_EXE_brain"))
            .args([
                "server",
                "run",
                "--generation",
                &generation.to_string(),
                "--port",
                "0",
            ])
            .env("HOME", home.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn hidden server");
        let handoff = election.handoff();
        let client = ServerClient::with_launch_context(
            paths.clone(),
            std::path::PathBuf::from(env!("CARGO_BIN_EXE_brain")),
            home.path().to_path_buf(),
        );
        wait_for("shared server reachability", || {
            client.connect_existing().is_ok()
        });
        handoff.cleanup().expect("finish election handoff");
        Self {
            child,
            _home: home,
            paths,
            client,
            generation,
        }
    }

    fn shutdown_with_two_leases(&mut self) {
        let family = lease(
            "family",
            "e806258e-491a-436d-9db4-a5ca9903e0d4",
            "57b162df-983a-45c3-ac7e-bad94eb27a99",
            "00000000-0000-0000-0000-000000000003",
        );
        let personal = lease(
            "personal",
            "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b",
            "91a0cfc2-7427-49d5-a2f1-258f985cd7e5",
            "00000000-0000-0000-0000-000000000004",
        );
        self.client.register(&family).expect("register family");
        self.client.register(&personal).expect("register personal");
        self.client
            .unregister(family.lease_id)
            .expect("unregister family");
        self.client
            .unregister(personal.lease_id)
            .expect("unregister personal");
        wait_for("shared server process exit", || {
            self.child.try_wait().ok().flatten().is_some()
        });
    }
}

fn prepare_workspace_registry(home: &std::path::Path) {
    let config = home.join(".config/brain");
    let family = home.join("family");
    let personal = home.join("personal");
    std::fs::create_dir_all(&config).expect("machine config");
    for (root, workspace_id, ingress_id) in [
        (
            &family,
            "e806258e-491a-436d-9db4-a5ca9903e0d4",
            "57b162df-983a-45c3-ac7e-bad94eb27a99",
        ),
        (
            &personal,
            "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b",
            "91a0cfc2-7427-49d5-a2f1-258f985cd7e5",
        ),
    ] {
        std::fs::create_dir_all(root.join(".config")).expect("workspace config");
        let manifest = serde_json::json!({
            "schema_version": 1,
            "workspace_id": workspace_id,
            "receiver_ingress_id": ingress_id,
            "minimum_brain_version": env!("CARGO_PKG_VERSION")
        });
        std::fs::write(
            root.join(".config/workspace.json"),
            serde_json::to_vec_pretty(&manifest).expect("manifest JSON"),
        )
        .expect("workspace manifest");
    }
    let registry = serde_json::json!({
        "schema_version": 2,
        "default_workspace": "personal",
        "workspaces": {
            "family": {
                "workspace_id": "e806258e-491a-436d-9db4-a5ca9903e0d4",
                "root": family,
                "aliases": [],
                "local_user_id": "tester",
                "receiver_enabled": true,
                "env": {}
            },
            "personal": {
                "workspace_id": "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b",
                "root": personal,
                "aliases": [],
                "local_user_id": "tester",
                "receiver_enabled": true,
                "env": {}
            }
        }
    });
    std::fs::write(
        config.join("env.json"),
        serde_json::to_vec_pretty(&registry).expect("registry JSON"),
    )
    .expect("machine registry");
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn wait_for(description: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {description}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
