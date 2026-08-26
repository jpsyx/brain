use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use brain::agent::{
    AgentObservationCursor, AgentObservationError, AgentObservationRequest, AgentSession,
};
use brain::state::{ReceiverJobToken, ReceiverObservationSet};

#[path = "receiver_observation_privacy/policy.rs"]
mod policy;

const TOKEN: &str = "11111111-1111-4111-8111-111111111111";
const INSTANCE: &str = "22222222-2222-4222-8222-222222222222";
const WORKSPACE: &str = "33333333-3333-4333-8333-333333333333";
const SESSION: &str = "privacy-native-session";
const SENDER_CANARY: &str = "sender-canary-cafe@private.corp";
const LOCAL_PATH_CANARY: &str = "/Users/private-runtime-owner/receiver-secret";
const PRIVATE_HOST_CANARY: &str = "https://receiver.runtime.private.lan/callback";
const PRIVATE_CANARIES: &[&str] = &[
    "prompt-canary-7e7b",
    "body-canary-8f8c",
    "response-canary-9a9d",
    "recipient-canary-acde",
    "credential-canary-bdef",
    SENDER_CANARY,
    LOCAL_PATH_CANARY,
    PRIVATE_HOST_CANARY,
];

#[test]
fn debug_errors_and_diagnostic_contracts_redact_tokens_and_private_content() {
    let job_token = ReceiverJobToken::parse(TOKEN).expect("job token");
    assert_eq!(format!("{job_token:?}"), "ReceiverJobToken(<redacted>)");
    let session = AgentSession::new(PRIVATE_CANARIES[0]).expect("native session");
    let request = AgentObservationRequest::new(
        TOKEN,
        INSTANCE,
        PathBuf::from(PRIVATE_CANARIES.join("/")),
        session,
        AgentObservationCursor::launched(),
    );
    assert_eq!(
        format!("{request:?}"),
        "AgentObservationRequest(<redacted>)"
    );
    let set = ReceiverObservationSet {
        token: job_token,
        instance: INSTANCE.to_owned(),
        session_id: PRIVATE_CANARIES[2].to_owned(),
        revision: 3,
        accepted_at_unix_ms: Some(1_000),
        progressing_at_unix_ms: Some(1_100),
        latest_progress_at_unix_ms: Some(1_100),
        completed_at_unix_ms: Some(1_200),
        authorized_at_unix_ms: 1_300,
    };
    assert_eq!(format!("{set:?}"), "ReceiverObservationSet(<redacted>)");
    for error in all_observation_errors() {
        assert_private_absent(&format!("{error:?}: {error}"), true);
    }
    for rendered in [
        format!("{request:?}"),
        format!("{set:?}"),
        format!("{job_token:?}"),
    ] {
        assert_private_absent(&rendered, true);
    }

    let diagnostic =
        std::fs::read_to_string(repository_root().join("src/tui/app_brain/receiver/diagnostic.rs"))
            .expect("diagnostic source");
    for private_field in [
        "token",
        "prompt",
        "body",
        "message",
        "response",
        "sender",
        "recipient",
        "credential",
        "path",
        "url",
        "uri",
        "host",
        "address",
        "snapshot",
        "transcript",
    ] {
        assert!(
            !diagnostic.contains(private_field),
            "diagnostic names {private_field}"
        );
    }
}

#[test]
fn submit_tool_and_stop_producers_keep_private_content_out_of_observations_and_output() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let observation = observation_path(&temporary, "observation.json");
    let state_db = temporary.path().join("state.db");
    let responses = temporary.path().join("responses");
    create_active_session(&state_db, "claude");

    let submit = serde_json::json!({
        "hook_event_name": "UserPromptSubmit",
        "session_id": SESSION,
        "prompt_id": "privacy-receiver-turn",
        "prompt": format!("{}\n<!-- brain:receiver-job-token={TOKEN} -->", PRIVATE_CANARIES[0]),
        "body": PRIVATE_CANARIES[1],
        "response": PRIVATE_CANARIES[2],
        "recipient": PRIVATE_CANARIES[3],
        "credential": PRIVATE_CANARIES[4],
        "sender": SENDER_CANARY,
        "local_path": LOCAL_PATH_CANARY,
        "private_host": PRIVATE_HOST_CANARY,
    });
    assert_safe_process(&run_bridge(&observation, "claude", &submit));
    assert_safe_snapshot(&observation);

    let tool = serde_json::json!({
        "hook_event_name": "PostToolUse",
        "session_id": SESSION,
        "prompt_id": "privacy-receiver-turn",
        "tool_use_id": "privacy-turn",
        "turn_id": "privacy-turn",
        "prompt": PRIVATE_CANARIES[0],
        "body": PRIVATE_CANARIES[1],
        "response": PRIVATE_CANARIES[2],
        "recipient": PRIVATE_CANARIES[3],
        "credential": PRIVATE_CANARIES[4],
        "sender": SENDER_CANARY,
        "local_path": LOCAL_PATH_CANARY,
        "private_host": PRIVATE_HOST_CANARY,
    });
    assert_safe_process(&run_bridge(&observation, "claude", &tool));
    assert_safe_snapshot(&observation);
    std::thread::sleep(std::time::Duration::from_millis(2));
    let later_tool = serde_json::json!({
        "hook_event_name": "PostToolUse",
        "session_id": SESSION,
        "prompt_id": "privacy-receiver-turn",
        "tool_use_id": "privacy-turn-later",
        "turn_id": "privacy-turn-later",
        "prompt": PRIVATE_CANARIES[0],
        "body": PRIVATE_CANARIES[1],
        "response": PRIVATE_CANARIES[2],
        "recipient": PRIVATE_CANARIES[3],
        "credential": PRIVATE_CANARIES[4],
        "sender": SENDER_CANARY,
        "local_path": LOCAL_PATH_CANARY,
        "private_host": PRIVATE_HOST_CANARY,
    });
    assert_safe_process(&run_bridge(&observation, "claude", &later_tool));
    assert_safe_snapshot(&observation);
    let pulsed: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&observation).unwrap()).unwrap();
    assert_eq!(pulsed["revision"], 3);
    assert_eq!(pulsed["turn_id"], "privacy-turn-later");
    assert!(pulsed["latest_progress_at_unix_ms"].as_u64().is_some());

    let stop = serde_json::json!({
        "session_id": SESSION,
        "turn_id": "privacy-turn-final",
        "last_assistant_message": PRIVATE_CANARIES[2],
        "prompt": PRIVATE_CANARIES[0],
        "body": PRIVATE_CANARIES[1],
        "recipient": PRIVATE_CANARIES[3],
        "credential": PRIVATE_CANARIES[4],
        "sender": SENDER_CANARY,
        "local_path": LOCAL_PATH_CANARY,
        "private_host": PRIVATE_HOST_CANARY,
    });
    let output = run_stop_hook(&observation, &state_db, &responses, "claude", &stop);
    assert_safe_process(&output);
    assert_safe_snapshot(&observation);
    let artifact: serde_json::Value = serde_json::from_slice(
        &std::fs::read(responses.join(format!("{INSTANCE}.json"))).expect("completion artifact"),
    )
    .expect("completion JSON");
    assert_trusted_completion_artifact(&artifact);
}

#[test]
fn opencode_plugin_submit_tool_and_idle_paths_do_not_log_or_snapshot_private_content() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let root = temporary.path().join("workspace");
    std::fs::create_dir_all(&root).expect("workspace root");
    let observation = observation_path(&temporary, "opencode-observation.json");
    let state_db = temporary.path().join("opencode-state.db");
    let responses = temporary.path().join("opencode-responses");
    create_active_session(&state_db, "opencode");
    let repository = repository_root();
    let output = Command::new(javascript_runtime())
        .arg(repository.join("tests/fixtures/opencode/plugin_harness.js"))
        .arg(repository.join("scripts/opencode_brain_plugin.js"))
        .arg("external_observation_privacy")
        .env("BRAIN_WORKSPACE_ID", WORKSPACE)
        .env("BRAIN_WORKSPACE", "privacy-workspace")
        .env("BRAIN_ROOT", &root)
        .env("BRAIN_ACTOR_ID", "privacy-actor")
        .env("BRAIN_CHANNEL", "email")
        .env("BRAIN_AGENT_KIND", "opencode")
        .env("BRAIN_INSTANCE_ID", INSTANCE)
        .env("BRAIN_PID", std::process::id().to_string())
        .env("BRAIN_STATE_DB", &state_db)
        .env("BRAIN_RESPONSE_DIR", &responses)
        .env("BRAIN_RESPONSE_ID", INSTANCE)
        .env("BRAIN_RECEIVER_JOB_TOKEN", TOKEN)
        .env("BRAIN_RECEIVER_OBSERVATION_PATH", &observation)
        .env("TEST_RECEIVER_SESSION_ID", SESSION)
        .env("TEST_PROMPT_CANARY", PRIVATE_CANARIES[0])
        .env("TEST_BODY_CANARY", PRIVATE_CANARIES[1])
        .env("TEST_RESPONSE_CANARY", PRIVATE_CANARIES[2])
        .env("TEST_RECIPIENT_CANARY", PRIVATE_CANARIES[3])
        .env("TEST_CREDENTIAL_CANARY", PRIVATE_CANARIES[4])
        .env("TEST_SENDER_CANARY", SENDER_CANARY)
        .env("TEST_LOCAL_PATH_CANARY", LOCAL_PATH_CANARY)
        .env("TEST_PRIVATE_HOST_CANARY", PRIVATE_HOST_CANARY)
        .output()
        .expect("run OpenCode privacy harness");
    assert!(
        output.status.success(),
        "OpenCode privacy harness failed: {output:?}"
    );
    assert_safe_process(&output);
    assert_safe_snapshot(&observation);
    let artifact: serde_json::Value = serde_json::from_slice(
        &std::fs::read(responses.join(format!("{INSTANCE}.json"))).expect("completion artifact"),
    )
    .expect("completion JSON");
    assert_trusted_completion_artifact(&artifact);
}

fn run_bridge(path: &Path, kind: &str, payload: &serde_json::Value) -> Output {
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

fn observation_path(temporary: &tempfile::TempDir, name: &str) -> PathBuf {
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

fn run_stop_hook(
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

fn create_active_session(path: &Path, kind: &str) {
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

fn assert_safe_snapshot(path: &Path) {
    let snapshot = std::fs::read_to_string(path).expect("observation snapshot");
    for canary in PRIVATE_CANARIES {
        assert!(!snapshot.contains(canary), "snapshot leaked {canary}");
    }
    let value: serde_json::Value = serde_json::from_str(&snapshot).expect("snapshot JSON");
    assert_eq!(value.as_object().expect("snapshot object").len(), 11);
    assert_eq!(value["job_token"], TOKEN);
    assert_eq!(value["instance_id"], INSTANCE);
    assert_eq!(value["session_id"], SESSION);
}

fn assert_trusted_completion_artifact(artifact: &serde_json::Value) {
    assert_eq!(artifact["message"], PRIVATE_CANARIES[2]);
    assert_eq!(artifact["job_token"], TOKEN);
    let serialized = artifact.to_string();
    for canary in PRIVATE_CANARIES
        .iter()
        .copied()
        .filter(|canary| *canary != PRIVATE_CANARIES[2])
    {
        assert!(
            !serialized.contains(canary),
            "completion artifact leaked {canary}"
        );
    }
}

fn assert_safe_process(output: &Output) {
    assert!(
        output.status.success(),
        "privacy producer failed: {output:?}"
    );
    assert_private_absent(&String::from_utf8_lossy(&output.stdout), true);
    assert_private_absent(&String::from_utf8_lossy(&output.stderr), true);
}

fn assert_private_absent(rendered: &str, include_token: bool) {
    for canary in PRIVATE_CANARIES {
        assert!(
            !rendered.contains(canary),
            "output leaked {canary}: {rendered}"
        );
    }
    if include_token {
        assert!(!rendered.contains(TOKEN), "output leaked token: {rendered}");
    }
}

const fn all_observation_errors() -> [AgentObservationError; 14] {
    [
        AgentObservationError::InvalidIdentifier,
        AgentObservationError::WrongPath,
        AgentObservationError::PlaceholderSession,
        AgentObservationError::OwnershipUnavailable,
        AgentObservationError::SessionOwnership,
        AgentObservationError::InvalidFileType,
        AgentObservationError::InvalidPermissions,
        AgentObservationError::SnapshotTooLarge,
        AgentObservationError::TruncatedSnapshot,
        AgentObservationError::MalformedSnapshot,
        AgentObservationError::IdentityMismatch,
        AgentObservationError::SessionMismatch,
        AgentObservationError::RevisionRegression,
        AgentObservationError::AmbiguousLifecycle,
    ]
}

fn javascript_runtime() -> &'static str {
    ["bun", "node"]
        .into_iter()
        .find(|runtime| Command::new(runtime).arg("--version").output().is_ok())
        .expect("OpenCode privacy requires Bun or Node")
}

fn repository_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}
