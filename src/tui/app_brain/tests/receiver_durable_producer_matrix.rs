use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use super::receiver_durable_support::accept_email_job;
use super::*;

use crate::state::ReceiverJobState;

#[derive(Clone, Copy)]
enum ProducerStage {
    ReorderedProgress,
    Accepted,
    Progressing,
    Completed,
}

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
        assert_job(
            &db,
            first.job_id(),
            ReceiverJobState::Launched,
            0,
            None,
            None,
        );

        produce_stage(
            &app,
            kind,
            &first_session,
            &first_path,
            ProducerStage::ReorderedProgress,
        );
        app.tick_receiver();
        assert_job(
            &db,
            first.job_id(),
            ReceiverJobState::Launched,
            0,
            None,
            None,
        );
        assert!(
            !first_path.exists(),
            "{kind:?} reordered progress created evidence"
        );

        produce_stage(
            &app,
            kind,
            &first_session,
            &first_path,
            ProducerStage::Accepted,
        );
        let accepted_snapshot = snapshot(&first_path);
        let accepted_at = snapshot_timestamp(&accepted_snapshot, "accepted_at_unix_ms");
        assert_eq!(accepted_snapshot["phase"], "accepted", "{kind:?}");
        assert_eq!(accepted_snapshot["revision"], 1, "{kind:?}");
        app.tick_receiver();
        assert_job(
            &db,
            first.job_id(),
            ReceiverJobState::Accepted,
            1,
            Some(accepted_at),
            None,
        );
        assert_duplicate_stage(
            &mut app,
            &db,
            first.job_id(),
            kind,
            &first_session,
            &first_path,
            ProducerStage::Accepted,
        );

        produce_stage(
            &app,
            kind,
            &first_session,
            &first_path,
            ProducerStage::Progressing,
        );
        let progressing_snapshot = snapshot(&first_path);
        let progressing_at = snapshot_timestamp(&progressing_snapshot, "progressing_at_unix_ms");
        assert_eq!(progressing_snapshot["phase"], "progressing", "{kind:?}");
        assert_eq!(progressing_snapshot["revision"], 2, "{kind:?}");
        assert_eq!(
            snapshot_timestamp(&progressing_snapshot, "accepted_at_unix_ms"),
            accepted_at,
            "{kind:?} rewrote the accepted timestamp"
        );
        app.tick_receiver();
        assert_job(
            &db,
            first.job_id(),
            ReceiverJobState::Processing,
            2,
            Some(accepted_at),
            Some(progressing_at),
        );
        assert_duplicate_stage(
            &mut app,
            &db,
            first.job_id(),
            kind,
            &first_session,
            &first_path,
            ProducerStage::Progressing,
        );

        produce_stage(
            &app,
            kind,
            &first_session,
            &first_path,
            ProducerStage::Completed,
        );
        let completed_snapshot = snapshot(&first_path);
        let completed_at = snapshot_timestamp(&completed_snapshot, "completed_at_unix_ms");
        assert_eq!(completed_snapshot["phase"], "completed", "{kind:?}");
        assert_eq!(completed_snapshot["revision"], 3, "{kind:?}");
        assert_eq!(
            snapshot_timestamp(&completed_snapshot, "accepted_at_unix_ms"),
            accepted_at,
            "{kind:?} rewrote the accepted timestamp"
        );
        assert_eq!(
            snapshot_timestamp(&completed_snapshot, "progressing_at_unix_ms"),
            progressing_at,
            "{kind:?} rewrote the progress timestamp"
        );
        assert_terminal_duplicate(&app, kind, &first_session, &first_path);
        app.tick_receiver();
        let completed = db.receiver_job(first.job_id()).unwrap().unwrap();
        assert_eq!(completed.state(), ReceiverJobState::Done, "{kind:?}");
        assert_eq!(completed.observation_revision(), 3, "{kind:?}");
        assert_eq!(
            completed.accepted_at_unix_ms(),
            Some(accepted_at),
            "{kind:?}"
        );
        assert_eq!(
            completed.progressing_at_unix_ms(),
            Some(progressing_at),
            "{kind:?}"
        );
        assert_eq!(
            completed.completed_at_unix_ms(),
            Some(completed_at),
            "{kind:?}"
        );
        assert_eq!(first_transport.shutdowns(), 1, "{kind:?}");

        let second_transport = TransportRecording::default();
        app.brain
            .replace_receiver_transport(second_transport.transport());
        app.tick_receiver();
        let second_session =
            rotate_active_session(&app, &format!("native-{}-second", kind.as_str()));
        let second_path = active_observation_path(&app);
        produce_stage(
            &app,
            kind,
            &second_session,
            &second_path,
            ProducerStage::Completed,
        );
        let completion_first = snapshot(&second_path);
        let completion_first_at = snapshot_timestamp(&completion_first, "completed_at_unix_ms");
        assert_eq!(completion_first["phase"], "completed", "{kind:?}");
        assert_eq!(completion_first["revision"], 1, "{kind:?}");
        assert!(
            completion_first["accepted_at_unix_ms"].is_null(),
            "{kind:?}"
        );
        assert!(
            completion_first["progressing_at_unix_ms"].is_null(),
            "{kind:?}"
        );
        assert_terminal_duplicate(&app, kind, &second_session, &second_path);
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
        assert_eq!(
            completion_first_job.completed_at_unix_ms(),
            Some(completion_first_at),
            "{kind:?}"
        );
        assert_eq!(second_transport.shutdowns(), 1, "{kind:?}");
        assert!(app.brain.receiver_run_observations().is_empty(), "{kind:?}");
    }
}

fn assert_terminal_duplicate(app: &App, kind: AgentKind, session: &AgentSession, path: &Path) {
    let completion_path = active_completion_path(app);
    let completion_before = std::fs::read(&completion_path).expect("completion");
    let snapshot_before = std::fs::read(path).expect("completed snapshot");
    produce_stage(app, kind, session, path, ProducerStage::Completed);
    assert_eq!(
        std::fs::read(&completion_path).expect("duplicate completion"),
        completion_before,
        "{kind:?} duplicate terminal producer changed the artifact"
    );
    assert_eq!(
        std::fs::read(path).expect("duplicate completed snapshot"),
        snapshot_before,
        "{kind:?} duplicate terminal producer changed the snapshot"
    );
}

fn assert_duplicate_stage(
    app: &mut App,
    db: &Db,
    job_id: crate::state::ReceiverJobId,
    kind: AgentKind,
    session: &AgentSession,
    path: &Path,
    stage: ProducerStage,
) {
    let durable_before = db.receiver_job(job_id).unwrap().unwrap();
    let snapshot_before = std::fs::read(path).expect("snapshot before duplicate");
    produce_stage(app, kind, session, path, stage);
    assert_eq!(
        std::fs::read(path).expect("snapshot after duplicate"),
        snapshot_before,
        "{kind:?} duplicate producer delivery changed the snapshot"
    );
    app.tick_receiver();
    assert_eq!(
        db.receiver_job(job_id).unwrap().unwrap(),
        durable_before,
        "{kind:?} duplicate producer delivery changed durable evidence"
    );
}

fn assert_job(
    db: &Db,
    job_id: crate::state::ReceiverJobId,
    state: ReceiverJobState,
    revision: u64,
    accepted_at: Option<u64>,
    progressing_at: Option<u64>,
) {
    let job = db.receiver_job(job_id).unwrap().unwrap();
    assert_eq!(job.state(), state);
    assert_eq!(job.observation_revision(), revision);
    assert_eq!(job.accepted_at_unix_ms(), accepted_at);
    assert_eq!(job.progressing_at_unix_ms(), progressing_at);
    assert_eq!(job.completed_at_unix_ms(), None);
}

fn produce_stage(
    app: &App,
    kind: AgentKind,
    session: &AgentSession,
    path: &Path,
    stage: ProducerStage,
) {
    if kind == AgentKind::OpenCode {
        run_opencode_stage(app, session, path, stage);
        return;
    }
    match stage {
        ProducerStage::ReorderedProgress => {
            run_bridge(
                app,
                kind,
                path,
                &progress_payload(kind, session, "turn-before"),
            );
        }
        ProducerStage::Accepted => {
            run_bridge(app, kind, path, &acceptance_payload(app, kind, session));
        }
        ProducerStage::Progressing => {
            run_bridge(
                app,
                kind,
                path,
                &progress_payload(kind, session, "turn-current"),
            );
        }
        ProducerStage::Completed => run_stop_hook(app, kind, session, path),
    }
}

fn run_opencode_stage(app: &App, session: &AgentSession, path: &Path, stage: ProducerStage) {
    let stage = match stage {
        ProducerStage::ReorderedProgress => "reordered_progress",
        ProducerStage::Accepted => "accepted",
        ProducerStage::Progressing => "progressing",
        ProducerStage::Completed => "completed",
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let active = app.receiver.active_durable_run().expect("active receiver");
    let scope = active.attribution.scope();
    let output = Command::new(javascript_runtime())
        .arg(root.join("tests/fixtures/opencode/plugin_harness.js"))
        .arg(root.join("scripts/opencode_brain_plugin.js"))
        .arg("external_observation_stage")
        .env("BRAIN_WORKSPACE_ID", scope.workspace_id().to_string())
        .env("BRAIN_WORKSPACE", app.context.workspace().name().as_str())
        .env("BRAIN_ROOT", app.context.workspace().root())
        .env("BRAIN_ACTOR_ID", scope.actor().user_id().as_str())
        .env("BRAIN_CHANNEL", scope.actor().channel().as_str())
        .env("BRAIN_AGENT_KIND", "opencode")
        .env("BRAIN_INSTANCE_ID", active.attribution.instance())
        .env("BRAIN_PID", std::process::id().to_string())
        .env("BRAIN_STATE_DB", app.context.state_db_path())
        .env(
            "BRAIN_RESPONSE_DIR",
            app.context.workspace().paths().responses_dir(),
        )
        .env("BRAIN_RESPONSE_ID", active.attribution.instance())
        .env(
            "BRAIN_RECEIVER_JOB_TOKEN",
            active.claim.job().token().to_string(),
        )
        .env("BRAIN_RECEIVER_OBSERVATION_PATH", path)
        .env("TEST_RECEIVER_SESSION_ID", session.as_str())
        .env("TEST_RECEIVER_STAGE", stage)
        .output()
        .expect("run OpenCode producer harness");
    assert_process_succeeded("OpenCode producer harness", &output);
}

pub(super) fn run_stop_hook(app: &App, kind: AgentKind, session: &AgentSession, path: &Path) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let active = app.receiver.active_durable_run().expect("active receiver");
    let scope = active.attribution.scope();
    let payload = if kind == AgentKind::Codex {
        serde_json::json!({
            "thread_id": session.as_str(),
            "last_assistant_message": "matrix completion",
            "turn_id": "matrix-turn-final",
        })
    } else {
        serde_json::json!({
            "session_id": session.as_str(),
            "last_assistant_message": "matrix completion",
            "turn_id": "matrix-turn-final",
        })
    };
    let mut child = Command::new("python3")
        .arg(root.join("scripts/agent_session_stop_hook.py"))
        .env("BRAIN_WORKSPACE_ID", scope.workspace_id().to_string())
        .env("BRAIN_ROOT", app.context.workspace().root())
        .env("BRAIN_ACTOR_ID", scope.actor().user_id().as_str())
        .env("BRAIN_CHANNEL", scope.actor().channel().as_str())
        .env("BRAIN_AGENT_KIND", kind.as_str())
        .env("BRAIN_INSTANCE_ID", active.attribution.instance())
        .env("BRAIN_STATE_DB", app.context.state_db_path())
        .env(
            "BRAIN_RESPONSE_DIR",
            app.context.workspace().paths().responses_dir(),
        )
        .env("BRAIN_RESPONSE_ID", active.attribution.instance())
        .env(
            "BRAIN_RECEIVER_JOB_TOKEN",
            active.claim.job().token().to_string(),
        )
        .env("BRAIN_RECEIVER_OBSERVATION_PATH", path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn stop hook");
    child
        .stdin
        .take()
        .expect("stop hook stdin")
        .write_all(payload.to_string().as_bytes())
        .expect("write stop payload");
    let output = child.wait_with_output().expect("wait for stop hook");
    assert_process_succeeded("stop hook", &output);
}

fn run_bridge(app: &App, kind: AgentKind, path: &Path, payload: &serde_json::Value) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut child = Command::new("python3")
        .arg(root.join("scripts/receiver_observation_bridge.py"))
        .env("BRAIN_AGENT_KIND", kind.as_str())
        .env("BRAIN_INSTANCE_ID", active_instance(app))
        .env("BRAIN_RECEIVER_JOB_TOKEN", active_job_token(app))
        .env("BRAIN_RECEIVER_OBSERVATION_PATH", path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn observation bridge");
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

fn acceptance_payload(app: &App, kind: AgentKind, session: &AgentSession) -> serde_json::Value {
    let marker = format!(
        "matrix\n<!-- brain:receiver-job-token={} -->",
        active_job_token(app)
    );
    if kind == AgentKind::Codex {
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
    }
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

fn snapshot(path: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(path).expect("observation snapshot"))
        .expect("valid observation snapshot")
}

pub(super) fn snapshot_timestamp(snapshot: &serde_json::Value, field: &str) -> u64 {
    snapshot[field].as_u64().expect("snapshot timestamp")
}

fn assert_process_succeeded(name: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{name} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn javascript_runtime() -> &'static str {
    ["bun", "node"]
        .into_iter()
        .find(|runtime| Command::new(runtime).arg("--version").output().is_ok())
        .expect("the OpenCode producer matrix requires Bun or Node")
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

pub(super) fn active_observation_path(app: &App) -> PathBuf {
    app.context
        .workspace()
        .paths()
        .receiver_observations_dir()
        .join(format!("{}.json", active_instance(app)))
}

pub(super) fn active_completion_path(app: &App) -> PathBuf {
    app.context
        .workspace()
        .paths()
        .responses_dir()
        .join(format!("{}.json", active_instance(app)))
}

pub(super) fn rotate_active_session(app: &App, session_id: &str) -> AgentSession {
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
