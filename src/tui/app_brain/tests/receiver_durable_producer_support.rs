use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use super::*;

const RECEIVER_TURN_ID: &str = "matrix-receiver-turn";

#[derive(Clone, Copy)]
pub(super) enum ProducerStage {
    ReorderedProgress,
    Accepted,
    Progressing,
    Completed,
}

pub(super) fn produce_stage(
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

pub(super) fn produce_completion(app: &App, kind: AgentKind, session: &AgentSession, path: &Path) {
    produce_stage(app, kind, session, path, ProducerStage::Completed);
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
            "turn_id": RECEIVER_TURN_ID,
            "prompt": marker,
        })
    } else {
        serde_json::json!({
            "hook_event_name": "UserPromptSubmit",
            "session_id": session.as_str(),
            "prompt_id": RECEIVER_TURN_ID,
            "prompt": marker,
        })
    }
}

fn progress_payload(kind: AgentKind, session: &AgentSession, turn: &str) -> serde_json::Value {
    if kind == AgentKind::Codex {
        serde_json::json!({
            "hook_event_name": "PostToolUse",
            "thread_id": session.as_str(),
            "turn_id": RECEIVER_TURN_ID,
            "tool_use_id": turn,
        })
    } else {
        serde_json::json!({
            "hook_event_name": "PostToolUse",
            "session_id": session.as_str(),
            "prompt_id": RECEIVER_TURN_ID,
            "tool_use_id": turn,
            "turn_id": turn,
        })
    }
}

pub(super) fn snapshot(path: &Path) -> serde_json::Value {
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
