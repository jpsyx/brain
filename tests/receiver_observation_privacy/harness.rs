use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::process::ExitStatusExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use super::{INSTANCE, PRIVATE_CANARIES, SESSION, TOKEN, WORKSPACE};

pub(super) fn run_bridge(path: &Path, kind: &str, payload: &serde_json::Value) -> Output {
    run_json_process(
        Command::new("python3")
            .arg(repository_root().join("scripts/receiver_observation_bridge.py"))
            .env("BRAIN_AGENT_KIND", kind)
            .env("BRAIN_RECEIVER_JOB_TOKEN", TOKEN)
            .env("BRAIN_RECEIVER_OBSERVATION_PATH", path)
            .env("BRAIN_INSTANCE_ID", INSTANCE),
        payload,
    )
}

pub(super) fn observation_path(temporary: &tempfile::TempDir, name: &str) -> PathBuf {
    let root = std::fs::canonicalize(temporary.path()).expect("canonical privacy directory");
    let cache = root.join("workspace-cache");
    let observations = cache.join("receiver-observations");
    std::fs::create_dir_all(&observations).expect("privacy observation directories");
    for directory in [&cache, &observations] {
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
            .expect("owner-only privacy observation directory");
    }
    observations.join(name)
}

pub(super) fn run_stop_hook(
    observation: &Path,
    state_db: &Path,
    responses: &Path,
    kind: &str,
    payload: &serde_json::Value,
) -> Output {
    run_json_process(
        Command::new("python3")
            .arg(repository_root().join("scripts/agent_session_stop_hook.py"))
            .env("BRAIN_WORKSPACE_ID", WORKSPACE)
            .env("BRAIN_ROOT", observation.parent().expect("privacy root"))
            .env("BRAIN_ACTOR_ID", "privacy-actor")
            .env("BRAIN_CHANNEL", "email")
            .env("BRAIN_AGENT_KIND", kind)
            .env("BRAIN_INSTANCE_ID", INSTANCE)
            .env("BRAIN_STATE_DB", state_db)
            .env("BRAIN_RESPONSE_DIR", responses)
            .env("BRAIN_RESPONSE_ID", INSTANCE)
            .env("BRAIN_RECEIVER_JOB_TOKEN", TOKEN)
            .env("BRAIN_RECEIVER_OBSERVATION_PATH", observation),
        payload,
    )
}

fn run_json_process(command: &mut Command, payload: &serde_json::Value) -> Output {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn privacy producer");
    child
        .stdin
        .take()
        .expect("privacy stdin")
        .write_all(payload.to_string().as_bytes())
        .expect("write privacy payload");
    child.wait_with_output().expect("wait privacy producer")
}

pub(super) fn create_active_session(path: &Path, kind: &str) {
    let connection = rusqlite::Connection::open(path).expect("privacy state DB");
    connection
        .execute_batch(
            "CREATE TABLE brain_sessions (
                agent_kind TEXT NOT NULL,
                agent_session_id TEXT NOT NULL,
                workspace_id TEXT NOT NULL,
                actor_id TEXT NOT NULL,
                channel TEXT NOT NULL,
                brain_instance_id TEXT NOT NULL,
                locked_pid INTEGER,
                completion_status TEXT NOT NULL
             );",
        )
        .expect("privacy session schema");
    connection
        .execute(
            "INSERT INTO brain_sessions VALUES (?1, ?2, ?3, 'privacy-actor', 'email', ?4, 42, 'active')",
            rusqlite::params![kind, SESSION, WORKSPACE, INSTANCE],
        )
        .expect("active privacy session");
}

pub(super) fn assert_safe_snapshot(path: &Path) {
    let snapshot = std::fs::read_to_string(path).expect("observation snapshot");
    for (index, canary) in PRIVATE_CANARIES.iter().enumerate() {
        assert!(
            !snapshot.contains(canary),
            "snapshot contains private canary at index {index}"
        );
    }
    let value: serde_json::Value = serde_json::from_str(&snapshot).expect("snapshot JSON");
    assert!(
        value.as_object().is_some_and(|object| object.len() == 11),
        "snapshot field count mismatch"
    );
    assert!(
        value["job_token"]
            .as_str()
            .is_some_and(|value| value == TOKEN),
        "snapshot token mismatch"
    );
    assert!(
        value["instance_id"].as_str() == Some(INSTANCE),
        "snapshot instance category mismatch"
    );
    assert!(
        value["session_id"].as_str() == Some(SESSION),
        "snapshot session category mismatch"
    );
}

pub(super) fn assert_trusted_completion_artifact(artifact: &serde_json::Value) {
    assert!(
        artifact["message"]
            .as_str()
            .is_some_and(|value| value == PRIVATE_CANARIES[2]),
        "completion artifact message mismatch"
    );
    assert!(
        artifact["job_token"]
            .as_str()
            .is_some_and(|value| value == TOKEN),
        "completion artifact token mismatch"
    );
    let serialized = artifact.to_string();
    for (index, canary) in PRIVATE_CANARIES
        .iter()
        .copied()
        .filter(|canary| *canary != PRIVATE_CANARIES[2])
        .enumerate()
    {
        assert!(
            !serialized.contains(canary),
            "completion artifact contains private canary at index {index}"
        );
    }
}

pub(super) fn assert_safe_process(output: &Output) {
    let success = output.status.success();
    let code_present = output.status.code().is_some();
    let signal_present = output.status.signal().is_some();
    let stdout_bytes = output.stdout.len();
    let stderr_bytes = output.stderr.len();
    assert!(
        success,
        "privacy producer failed: success={success}, code_present={code_present}, signal_present={signal_present}, stdout_bytes={stdout_bytes}, stderr_bytes={stderr_bytes}",
    );
    assert_private_absent(
        "privacy producer stdout",
        &String::from_utf8_lossy(&output.stdout),
        true,
    );
    assert_private_absent(
        "privacy producer stderr",
        &String::from_utf8_lossy(&output.stderr),
        true,
    );
}

pub(super) fn assert_private_absent(label: &str, rendered: &str, include_token: bool) {
    for (index, canary) in PRIVATE_CANARIES.iter().enumerate() {
        assert!(
            !rendered.contains(canary),
            "{label} contains private canary at index {index}"
        );
    }
    if include_token {
        assert!(
            !rendered.contains(TOKEN),
            "{label} contains a receiver token"
        );
    }
}

pub(super) fn javascript_runtime() -> &'static str {
    ["bun", "node"]
        .into_iter()
        .find(|runtime| Command::new(runtime).arg("--version").output().is_ok())
        .expect("OpenCode privacy requires Bun or Node")
}

pub(super) fn repository_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}
