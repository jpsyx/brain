use std::path::PathBuf;
use std::process::Command;

use brain::agent::{
    AgentObservationCursor, AgentObservationError, AgentObservationRequest, AgentSession,
};
use brain::state::{ReceiverJobToken, ReceiverObservationSet};

#[path = "receiver_observation_privacy/debug.rs"]
mod debug;
#[path = "receiver_observation_privacy/harness.rs"]
mod harness;
#[path = "receiver_observation_privacy/policy.rs"]
mod policy;

use harness::*;

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
    let rendered_job_token = format!("{job_token:?}");
    assert_private_absent("job token Debug", &rendered_job_token, true);
    assert!(
        rendered_job_token == "ReceiverJobToken(<redacted>)",
        "job token Debug shape mismatch"
    );
    let session = AgentSession::new(PRIVATE_CANARIES[0]).expect("native session");
    let request = AgentObservationRequest::new(
        TOKEN,
        INSTANCE,
        PathBuf::from(PRIVATE_CANARIES.join("/")),
        session,
        AgentObservationCursor::launched(),
    );
    let rendered_request = format!("{request:?}");
    assert_private_absent("observation request Debug", &rendered_request, true);
    assert!(
        rendered_request == "AgentObservationRequest(<redacted>)",
        "observation request Debug shape mismatch"
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
    let rendered_set = format!("{set:?}");
    assert_private_absent("observation set Debug", &rendered_set, true);
    assert!(
        rendered_set == "ReceiverObservationSet(<redacted>)",
        "observation set Debug shape mismatch"
    );
    for error in all_observation_errors() {
        assert_private_absent("observation error", &format!("{error:?}: {error}"), true);
    }
    for rendered in [
        format!("{request:?}"),
        format!("{set:?}"),
        format!("{job_token:?}"),
    ] {
        assert_private_absent("observation debug", &rendered, true);
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
    assert!(
        pulsed["revision"].as_u64() == Some(3),
        "progress snapshot revision mismatch"
    );
    assert!(
        pulsed["turn_id"].as_str() == Some("privacy-turn-later"),
        "progress snapshot turn category mismatch"
    );
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
    assert_safe_process(&output);
    assert_safe_snapshot(&observation);
    let artifact: serde_json::Value = serde_json::from_slice(
        &std::fs::read(responses.join(format!("{INSTANCE}.json"))).expect("completion artifact"),
    )
    .expect("completion JSON");
    assert_trusted_completion_artifact(&artifact);
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
