use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

pub(super) const JOB_TOKEN: &str = "11111111-1111-4111-8111-111111111111";
pub(super) const INSTANCE_ID: &str = "22222222-2222-4222-8222-222222222222";
pub(super) const SESSION_ID: &str = "receiver-session-1";

pub(super) fn bridge_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("receiver_observation_bridge.py")
}

pub(super) fn observation_directory(temporary: &tempfile::TempDir) -> PathBuf {
    let root = std::fs::canonicalize(temporary.path()).expect("canonical temporary directory");
    let cache = root.join("workspace-cache");
    let observations = cache.join("receiver-observations");
    std::fs::create_dir_all(&observations).expect("observation directories");
    for directory in [&cache, &observations] {
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
            .expect("owner-only observation directory");
    }
    observations
}

pub(super) fn observation_path(temporary: &tempfile::TempDir, name: impl AsRef<Path>) -> PathBuf {
    observation_directory(temporary).join(name)
}

pub(super) fn run_bridge(snapshot: &Path, payload: &serde_json::Value) -> Output {
    run_bridge_with_args(snapshot, payload, &[])
}

pub(super) fn run_required_bridge(snapshot: &Path, payload: &serde_json::Value) -> Output {
    run_bridge_with_args(snapshot, payload, &["--require-write"])
}

fn run_bridge_with_args(
    snapshot: &Path,
    payload: &serde_json::Value,
    arguments: &[&str],
) -> Output {
    let mut command = Command::new("python3");
    command
        .arg(bridge_path())
        .args(arguments)
        .env("BRAIN_RECEIVER_JOB_TOKEN", JOB_TOKEN)
        .env("BRAIN_RECEIVER_OBSERVATION_PATH", snapshot)
        .env("BRAIN_INSTANCE_ID", INSTANCE_ID)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("spawn observation bridge");
    child
        .stdin
        .take()
        .expect("bridge stdin")
        .write_all(payload.to_string().as_bytes())
        .expect("write bridge payload");
    child.wait_with_output().expect("wait observation bridge")
}

pub(super) fn run_bridge_with_setup(
    snapshot: &Path,
    payload: &serde_json::Value,
    setup: &str,
    environment: &[(&str, &Path)],
) -> Output {
    run_bridge_with_setup_and_mode(snapshot, payload, setup, environment, false)
}

pub(super) fn run_required_bridge_with_setup(
    snapshot: &Path,
    payload: &serde_json::Value,
    setup: &str,
) -> Output {
    run_bridge_with_setup_and_mode(snapshot, payload, setup, &[], true)
}

fn run_bridge_with_setup_and_mode(
    snapshot: &Path,
    payload: &serde_json::Value,
    setup: &str,
    environment: &[(&str, &Path)],
    required: bool,
) -> Output {
    let required = if required { "True" } else { "False" };
    let script = format!(
        r#"
import importlib.util
import os

spec = importlib.util.spec_from_file_location("brain_receiver_bridge", os.environ["BRAIN_TEST_BRIDGE"])
bridge = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bridge)
{setup}
os.umask(0o077)
try:
    succeeded = bridge.main()
except Exception:
    succeeded = False
if {required}:
    raise SystemExit(0 if succeeded else 1)
"#
    );
    let mut command = Command::new("python3");
    command
        .args(["-c", &script])
        .env("BRAIN_TEST_BRIDGE", bridge_path())
        .env("BRAIN_RECEIVER_JOB_TOKEN", JOB_TOKEN)
        .env("BRAIN_RECEIVER_OBSERVATION_PATH", snapshot)
        .env("BRAIN_INSTANCE_ID", INSTANCE_ID)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (name, value) in environment {
        command.env(name, value);
    }
    let mut child = command.spawn().expect("spawn configured bridge");
    child
        .stdin
        .take()
        .expect("bridge stdin")
        .write_all(payload.to_string().as_bytes())
        .expect("write bridge payload");
    child.wait_with_output().expect("wait configured bridge")
}

pub(super) fn accepted_payload(prompt: &str) -> serde_json::Value {
    serde_json::json!({
        "hook_event_name": "UserPromptSubmit",
        "session_id": SESSION_ID,
        "prompt": prompt,
    })
}

pub(super) fn snapshot(path: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(path).expect("observation snapshot"))
        .expect("snapshot JSON")
}

pub(super) fn progress_payload(session_id: &str, turn_id: &str) -> serde_json::Value {
    serde_json::json!({
        "hook_event_name": "PostToolUse",
        "session_id": session_id,
        "turn_id": turn_id,
    })
}

pub(super) fn spawn_bridge(snapshot: &Path, payload: &serde_json::Value) -> std::process::Child {
    let mut command = Command::new("python3");
    command
        .arg(bridge_path())
        .env("BRAIN_RECEIVER_JOB_TOKEN", JOB_TOKEN)
        .env("BRAIN_RECEIVER_OBSERVATION_PATH", snapshot)
        .env("BRAIN_INSTANCE_ID", INSTANCE_ID)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("spawn concurrent bridge");
    child
        .stdin
        .take()
        .expect("concurrent bridge stdin")
        .write_all(payload.to_string().as_bytes())
        .expect("write concurrent bridge payload");
    child
}
