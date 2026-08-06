use brain::server::lifecycle::{
    ElectionGuard, ProcessRecord, ServerGeneration, ServerPaths, StartDecision, decide_start,
    validate_election_token,
};
use clap::Parser as _;
use std::process::{Command, Stdio};

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
