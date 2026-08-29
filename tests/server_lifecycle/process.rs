use brain::server::lifecycle::{
    ElectionGuard, ProcessRecord, ServerDecision, ServerGeneration, ServerPaths, connect_or_elect,
};
use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixListener;
use std::process::{Command, Stdio};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use super::support::{LiveTui, PROCESS_FIXTURE_PERMITS, RunningServer, wait_for};

const TRANSITION_HELPER_HOME: &str = "BRAIN_TEST_TRANSITION_HELPER_HOME";
const TRANSITION_HELPER_DB: &str = "BRAIN_TEST_TRANSITION_HELPER_DB";

#[test]
fn durable_transition_is_visible_through_compiled_server_logs() {
    let home = tempfile::tempdir().expect("temporary server home");
    let paths = ServerPaths::from_home(home.path());
    std::fs::create_dir_all(paths.directory()).expect("create server stream directory");
    let state_db = home.path().join("transition-state.db");
    let helper = Command::new(std::env::current_exe().expect("current integration test binary"))
        .args(["--exact", "process::compiled_durable_transition_helper"])
        .env("HOME", home.path())
        .env(TRANSITION_HELPER_HOME, home.path())
        .env(TRANSITION_HELPER_DB, &state_db)
        .output()
        .expect("run compiled transition helper");
    assert!(helper.status.success(), "compiled transition helper failed");

    let output = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(["server", "logs"])
        .env("HOME", home.path())
        .env("NO_COLOR", "1")
        .output()
        .expect("run compiled server logs command");
    assert!(
        output.status.success(),
        "compiled server logs command failed"
    );
    let stdout = String::from_utf8(output.stdout).expect("server logs UTF-8");

    assert!(
        stdout.contains("receiver lifecycle event=ingress phase=queued queue_depth=1"),
        "server logs omitted the committed ingress transition"
    );
}

#[test]
fn compiled_durable_transition_helper() {
    let Some(_home) = std::env::var_os(TRANSITION_HELPER_HOME) else {
        return;
    };
    let state_db = std::env::var_os(TRANSITION_HELPER_DB).expect("transition helper state DB");
    let workspace_id = brain::workspace::WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b")
        .expect("workspace ID");
    let user_id = brain::users::UserId::parse("test-user").expect("user ID");
    let actor: brain::actor::ActorContext = serde_json::from_value(serde_json::json!({
        "user_id": user_id.as_str(),
        "display_name": "Test user",
        "channel": "sms"
    }))
    .expect("actor context");
    let job = brain::server::receiver::InboundJob {
        job_id: uuid::Uuid::new_v4(),
        workspace_id,
        actor,
        channel: brain::server::receiver::Channel::Sms,
        authenticated_sender: "+12125550100".to_owned(),
        response_sender: "+12125550100".to_owned(),
        prompt: "Transition fixture".to_owned(),
        attachments: Vec::new(),
        received_at_unix_ms: 100,
        provider_id: Some("transition-fixture".to_owned()),
        thread_participants: vec!["+12125550100".to_owned()],
        response_email: None,
        allowed_response_recipients: Vec::new(),
        email_reply: None,
    };
    let db = brain::state::Db::open_path_with_legacy_identity(
        std::path::Path::new(&state_db),
        &workspace_id.to_string(),
        user_id.as_str(),
    )
    .expect("transition helper state");
    let identity = brain::state::ReceiverConversationIdentity::sms(workspace_id, user_id);

    db.accept_receiver_job(&job, &identity)
        .expect("commit ingress transition");
}

#[test]
fn connect_or_elect_reuses_an_existing_generation() {
    let mut server = RunningServer::start();

    let connected = connect_or_elect(&server.client).expect("reuse running server");

    assert_eq!(connected.generation, server.generation);
    server.shutdown_with_two_leases();
}

#[test]
fn connect_or_elect_continues_after_an_older_server_generation_exits() {
    let _process_permit = PROCESS_FIXTURE_PERMITS.acquire();
    let home = tempfile::tempdir().expect("temporary server home");
    let paths = ServerPaths::from_home(home.path());
    let legacy_generation = ServerGeneration::new();
    let election = ElectionGuard::try_acquire(&paths, legacy_generation)
        .expect("legacy election probe")
        .expect("test process wins legacy election");
    let record = ProcessRecord {
        pid: std::process::id(),
        port: 0,
        generation: legacy_generation,
        started_at: "2026-08-29T00:00:00Z".to_owned(),
    };
    std::fs::write(
        paths.process_record(),
        serde_json::to_vec(&record).expect("legacy process record JSON"),
    )
    .expect("legacy process record");
    let listener = UnixListener::bind(paths.control_socket()).expect("legacy control socket");
    let legacy_paths = paths.clone();
    let legacy = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("legacy control request");
        let mut request = Vec::new();
        stream
            .read_to_end(&mut request)
            .expect("legacy control request bytes");
        assert!(!request.is_empty());
        writeln!(
            stream,
            "{{\"result\":\"snapshot\",\"generation\":\"{legacy_generation}\",\"live_leases\":1}}"
        )
        .expect("legacy snapshot");
        drop(stream);
        drop(listener);
        std::fs::remove_file(legacy_paths.control_socket()).expect("retire legacy socket");
        std::fs::remove_file(legacy_paths.process_record()).expect("retire legacy record");
        drop(election);
    });
    let client = brain::server::control::ServerClient::with_launch_context(
        paths.clone(),
        std::path::PathBuf::from(env!("CARGO_BIN_EXE_brain")),
        home.path().to_path_buf(),
    );

    let replacement = connect_or_elect(&client).expect("elect after legacy generation exits");

    legacy.join().expect("legacy server thread");
    assert_ne!(replacement.generation, legacy_generation);
    let status = Command::new("kill")
        .args(["-TERM", &replacement.pid.to_string()])
        .status()
        .expect("stop replacement server");
    assert!(status.success());
    wait_for("replacement generation cleanup", || {
        !paths.process_record().exists()
            && !paths.control_socket().exists()
            && !paths.election_lock().exists()
    });
}

#[test]
fn a_continuously_live_older_server_is_fenced_without_lease_mutation_or_election() {
    let _process_permit = PROCESS_FIXTURE_PERMITS.acquire();
    let home = tempfile::tempdir().expect("temporary server home");
    let paths = ServerPaths::from_home(home.path());
    std::fs::create_dir_all(paths.directory()).expect("server directory");
    let legacy_generation = ServerGeneration::new();
    let record = ProcessRecord {
        pid: std::process::id(),
        port: 0,
        generation: legacy_generation,
        started_at: "2026-08-29T00:00:00Z".to_owned(),
    };
    let record_bytes = serde_json::to_vec(&record).expect("legacy process record JSON");
    std::fs::write(paths.process_record(), &record_bytes).expect("legacy process record");
    let listener = UnixListener::bind(paths.control_socket()).expect("legacy control socket");
    listener
        .set_nonblocking(true)
        .expect("nonblocking legacy listener");
    let stop = Arc::new(AtomicBool::new(false));
    let stop_server = Arc::clone(&stop);
    let requests = Arc::new(Mutex::new(Vec::new()));
    let observed_requests = Arc::clone(&requests);
    let legacy = std::thread::spawn(move || {
        while !stop_server.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut request = Vec::new();
                    stream
                        .read_to_end(&mut request)
                        .expect("legacy control request bytes");
                    observed_requests.lock().expect("legacy requests").push(
                        serde_json::from_slice::<serde_json::Value>(&request)
                            .expect("request JSON"),
                    );
                    writeln!(
                        stream,
                        "{{\"result\":\"snapshot\",\"generation\":\"{legacy_generation}\",\"live_leases\":1}}"
                    )
                    .expect("legacy snapshot");
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("legacy accept failed: {error}"),
            }
        }
    });
    let client = brain::server::control::ServerClient::with_launch_context(
        paths.clone(),
        std::path::PathBuf::from(env!("CARGO_BIN_EXE_brain")),
        home.path().to_path_buf(),
    );
    let started = Instant::now();

    let result = connect_or_elect(&client);
    let record_after_fence = std::fs::read(paths.process_record());
    let election_after_fence = paths.election_lock().exists();

    stop.store(true, Ordering::Release);
    legacy.join().expect("legacy server thread");
    if let Ok(replacement) = &result {
        let _ = Command::new("kill")
            .args(["-TERM", &replacement.pid.to_string()])
            .status();
    }
    assert!(started.elapsed() <= Duration::from_secs(3));
    let error = result.expect_err("a live legacy generation must not be replaced");
    assert_eq!(
        error.to_string(),
        "🔴 Brain server protocol changed. Close every Brain TUI, then restart Brain."
    );
    assert_eq!(
        record_after_fence.expect("unchanged legacy process record"),
        record_bytes
    );
    let requests = requests.lock().expect("legacy requests");
    assert!(!requests.is_empty());
    assert!(
        requests
            .iter()
            .all(|request| request["action"] == "snapshot")
    );
    drop(requests);
    assert!(!election_after_fence);
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
