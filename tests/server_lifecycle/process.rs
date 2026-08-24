use brain::server::lifecycle::{
    ElectionGuard, ServerDecision, ServerGeneration, ServerPaths, connect_or_elect,
};
use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixListener;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::support::{LiveTui, PROCESS_FIXTURE_PERMITS, RunningServer, wait_for};

#[test]
fn connect_or_elect_reuses_an_existing_generation() {
    let mut server = RunningServer::start();

    let connected = connect_or_elect(&server.client).expect("reuse running server");

    assert_eq!(connected.generation, server.generation);
    server.shutdown_with_two_leases();
}

#[test]
fn elected_starter_exit_with_retained_token_retries_within_original_deadline() {
    elected_starter_exit_retries(false);
}

#[test]
fn elected_starter_exit_after_token_removal_retries_within_original_deadline() {
    elected_starter_exit_retries(true);
}

fn elected_starter_exit_retries(remove_token: bool) {
    use std::os::unix::fs::PermissionsExt as _;

    let _process_permit = PROCESS_FIXTURE_PERMITS.acquire();
    let home = tempfile::tempdir().expect("temporary server home");
    let paths = ServerPaths::from_home(home.path());
    let wrapper = home.path().join("elected-starter");
    let token_action = if remove_token {
        "rm -f \"$HOME/.cache/brain/server/election.lock\""
    } else {
        ":"
    };
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\ncount_file=\"$HOME/starter-count\"\ncount=0\n[ -f \"$count_file\" ] && count=$(cat \"$count_file\")\ncount=$((count + 1))\nprintf '%s\\n' \"$count\" > \"$count_file\"\nif [ \"$count\" -eq 1 ]; then\n  {token_action}\n  exit 19\nfi\nexec '{}' \"$@\"\n",
            env!("CARGO_BIN_EXE_brain")
        ),
    )
    .expect("write elected starter wrapper");
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700))
        .expect("make elected starter executable");
    let client = brain::server::control::ServerClient::with_launch_context(
        paths.clone(),
        wrapper,
        home.path().to_path_buf(),
    );

    let record = connect_or_elect(&client).expect("retry exited elected starter");

    let starter_count = std::fs::read_to_string(home.path().join("starter-count"))
        .expect("read starter count")
        .trim()
        .parse::<u32>()
        .expect("starter count is numeric");
    assert!(
        starter_count >= 2,
        "the deliberately failed starter must be retried"
    );
    let status = Command::new("kill")
        .args(["-TERM", &record.pid.to_string()])
        .status()
        .expect("stop replacement server");
    assert!(status.success());
    let deadline = Instant::now() + Duration::from_secs(2);
    while paths.process_record().exists()
        || paths.control_socket().exists()
        || paths.election_lock().exists()
    {
        assert!(Instant::now() < deadline, "replacement cleanup timed out");
        std::thread::yield_now();
    }
}

#[test]
fn final_unregister_stops_the_process_and_removes_generation_artifacts() {
    let mut server = RunningServer::start();
    let family = LiveTui::new(
        server.home(),
        "family",
        "e806258e-491a-436d-9db4-a5ca9903e0d4",
        "57b162df-983a-45c3-ac7e-bad94eb27a99",
        "00000000-0000-0000-0000-000000000001",
    );
    let personal = LiveTui::new(
        server.home(),
        "personal",
        "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b",
        "91a0cfc2-7427-49d5-a2f1-258f985cd7e5",
        "00000000-0000-0000-0000-000000000002",
    );
    server
        .client
        .register_generation(&family.registration(server.generation))
        .expect("register family");
    server
        .client
        .register_generation(&personal.registration(server.generation))
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
    let personal = LiveTui::new(
        server.home(),
        "personal",
        "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b",
        "91a0cfc2-7427-49d5-a2f1-258f985cd7e5",
        "00000000-0000-0000-0000-000000000005",
    );
    server
        .client
        .register_generation(&personal.registration(server.generation))
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
    let _process_permit = PROCESS_FIXTURE_PERMITS.acquire();
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
    let _process_permit = PROCESS_FIXTURE_PERMITS.acquire();
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
