use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use super::receiver_durable_support::accept_email_job;
use super::*;

use crate::state::ReceiverJobState;

#[test]
fn normalized_producers_drive_one_controller_and_coordinator_lifecycle_matrix() {
    for kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&temporary, &cli, kind);
        app.receiver.record_intent(true);
        let db = Db::open(app.context.workspace()).expect("state DB");
        let first = accept_email_job(&app, &db, "synthetic lifecycle", 100);
        let second = accept_email_job(&app, &db, "synthetic completion", 200);
        let first_transport = TransportRecording::default();
        app.brain
            .replace_receiver_transport(first_transport.transport());
        app.tick_receiver();
        let first_session = rotate_active_session(&app, &format!("native-{}-first", kind.as_str()));
        let first_path = active_observation_path(&app);

        produce_progressing_snapshot(&app, kind, &first_session, &first_path);
        let progressing_before_duplicate = std::fs::read(&first_path).expect("progress snapshot");
        produce_progressing_snapshot(&app, kind, &first_session, &first_path);
        assert_eq!(
            std::fs::read(&first_path).expect("duplicate progress snapshot"),
            progressing_before_duplicate,
            "{kind:?} duplicate producer delivery changed the snapshot"
        );

        app.tick_receiver();
        let progressing = db.receiver_job(first.job_id()).unwrap().unwrap();
        assert_eq!(
            progressing.state(),
            ReceiverJobState::Processing,
            "{kind:?}"
        );
        assert_eq!(progressing.observation_revision(), 2, "{kind:?}");
        assert!(progressing.accepted_at_unix_ms().is_some(), "{kind:?}");
        assert!(progressing.progressing_at_unix_ms().is_some(), "{kind:?}");

        let completion = completion_payload(kind, &first_session);
        run_bridge(&app, kind, &first_path, &completion);
        let completed_before_duplicate = std::fs::read(&first_path).expect("completed snapshot");
        run_bridge(&app, kind, &first_path, &completion);
        assert_eq!(
            std::fs::read(&first_path).expect("duplicate completed snapshot"),
            completed_before_duplicate,
            "{kind:?} duplicate completion changed the snapshot"
        );

        app.tick_receiver();
        let completed = db.receiver_job(first.job_id()).unwrap().unwrap();
        assert_eq!(completed.state(), ReceiverJobState::Done, "{kind:?}");
        assert_eq!(completed.observation_revision(), 3, "{kind:?}");
        assert_eq!(first_transport.shutdowns(), 1, "{kind:?}");

        let second_transport = TransportRecording::default();
        app.brain
            .replace_receiver_transport(second_transport.transport());
        app.tick_receiver();
        let second_session =
            rotate_active_session(&app, &format!("native-{}-second", kind.as_str()));
        let second_path = active_observation_path(&app);
        let completion_first = completion_payload(kind, &second_session);
        run_bridge(&app, kind, &second_path, &completion_first);
        let completion_first_before_duplicate =
            std::fs::read(&second_path).expect("completion-first snapshot");
        run_bridge(&app, kind, &second_path, &completion_first);
        assert_eq!(
            std::fs::read(&second_path).expect("duplicate completion-first snapshot"),
            completion_first_before_duplicate,
            "{kind:?} duplicate completion-first delivery changed the snapshot"
        );

        app.tick_receiver();
        let completion_first_job = db.receiver_job(second.job_id()).unwrap().unwrap();
        assert_eq!(
            completion_first_job.state(),
            ReceiverJobState::Done,
            "{kind:?}"
        );
        assert_eq!(completion_first_job.observation_revision(), 1, "{kind:?}");
        assert_eq!(completion_first_job.accepted_at_unix_ms(), None, "{kind:?}");
        assert_eq!(
            completion_first_job.progressing_at_unix_ms(),
            None,
            "{kind:?}"
        );
        assert!(
            completion_first_job.completed_at_unix_ms().is_some(),
            "{kind:?}"
        );
        assert_eq!(second_transport.shutdowns(), 1, "{kind:?}");
        assert!(app.brain.receiver_run_observations().is_empty(), "{kind:?}");
    }
}

fn produce_progressing_snapshot(app: &App, kind: AgentKind, session: &AgentSession, path: &Path) {
    if kind == AgentKind::OpenCode {
        run_opencode_producer(app, session, path);
        return;
    }
    if !path.exists() {
        run_bridge(
            app,
            kind,
            path,
            &progress_payload(kind, session, "turn-before"),
        );
        assert!(!path.exists(), "{kind:?} progress created acceptance");
    }
    let marker = format!(
        "synthetic\n<!-- brain:receiver-job-token={} -->",
        active_job_token(app)
    );
    let accepted = if kind == AgentKind::Codex {
        serde_json::json!({
            "hook_event_name": "UserPromptSubmit",
            "thread_id": session.as_str(),
            "prompt": marker,
        })
    } else {
        serde_json::json!({
            "hook_event_name": "UserPromptSubmit",
            "session_id": session.as_str(),
            "prompt": marker,
        })
    };
    run_bridge(app, kind, path, &accepted);
    run_bridge(app, kind, path, &accepted);
    let progress = progress_payload(kind, session, "turn-current");
    run_bridge(app, kind, path, &progress);
    run_bridge(app, kind, path, &progress);
}

fn run_opencode_producer(app: &App, session: &AgentSession, path: &Path) {
    let runtime = ["bun", "node"]
        .into_iter()
        .find(|runtime| Command::new(runtime).arg("--version").output().is_ok())
        .expect("the OpenCode producer matrix requires Bun or Node");
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new(runtime)
        .arg(root.join("tests/fixtures/opencode/plugin_harness.js"))
        .arg(root.join("scripts/opencode_brain_plugin.js"))
        .arg("external_observation")
        .env("BRAIN_ROOT", app.context.workspace().root())
        .env("BRAIN_AGENT_KIND", "opencode")
        .env("BRAIN_INSTANCE_ID", active_instance(app))
        .env("BRAIN_RECEIVER_JOB_TOKEN", active_job_token(app))
        .env("BRAIN_RECEIVER_OBSERVATION_PATH", path)
        .env("TEST_RECEIVER_SESSION_ID", session.as_str())
        .output()
        .expect("run OpenCode producer harness");
    assert_process_succeeded("OpenCode producer harness", &output);
}

fn run_bridge(app: &App, kind: AgentKind, path: &Path, payload: &serde_json::Value) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut command = Command::new("python3");
    command
        .arg(root.join("scripts/receiver_observation_bridge.py"))
        .env("BRAIN_AGENT_KIND", kind.as_str())
        .env("BRAIN_INSTANCE_ID", active_instance(app))
        .env("BRAIN_RECEIVER_JOB_TOKEN", active_job_token(app))
        .env("BRAIN_RECEIVER_OBSERVATION_PATH", path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("spawn observation bridge");
    child
        .stdin
        .take()
        .expect("observation bridge stdin")
        .write_all(payload.to_string().as_bytes())
        .expect("write observation payload");
    let output = child
        .wait_with_output()
        .expect("wait for observation bridge");
    assert_process_succeeded("observation bridge", &output);
}

fn assert_process_succeeded(name: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{name} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn progress_payload(kind: AgentKind, session: &AgentSession, turn: &str) -> serde_json::Value {
    if kind == AgentKind::Codex {
        serde_json::json!({
            "hook_event_name": "PostToolUse",
            "thread_id": session.as_str(),
            "turn_id": turn,
        })
    } else {
        serde_json::json!({
            "hook_event_name": "PostToolUse",
            "session_id": session.as_str(),
            "turn_id": turn,
        })
    }
}

fn completion_payload(kind: AgentKind, session: &AgentSession) -> serde_json::Value {
    if kind == AgentKind::Codex {
        serde_json::json!({
            "hook_event_name": "Stop",
            "thread_id": session.as_str(),
            "turn_id": "turn-final",
        })
    } else {
        serde_json::json!({
            "hook_event_name": "Stop",
            "session_id": session.as_str(),
            "turn_id": "turn-final",
        })
    }
}

fn active_instance(app: &App) -> String {
    app.receiver
        .active_durable_run()
        .expect("active receiver")
        .attribution
        .instance()
        .to_owned()
}

fn active_job_token(app: &App) -> String {
    app.receiver
        .active_durable_run()
        .expect("active receiver")
        .claim
        .job()
        .token()
        .to_string()
}

fn active_observation_path(app: &App) -> PathBuf {
    app.context
        .workspace()
        .paths()
        .receiver_observations_dir()
        .join(format!("{}.json", active_instance(app)))
}

fn rotate_active_session(app: &App, session_id: &str) -> AgentSession {
    let session = AgentSession::new(session_id).expect("native session");
    rusqlite::Connection::open(app.context.state_db_path())
        .expect("lifecycle fixture connection")
        .execute(
            "UPDATE brain_sessions SET agent_session_id = ?1 WHERE brain_instance_id = ?2",
            rusqlite::params![session.as_str(), active_instance(app)],
        )
        .expect("simulate lifecycle native session");
    session
}
