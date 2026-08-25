use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use brain::agent::{
    AgentObservationCursor, AgentObservationError, AgentObservationRequest, AgentSession,
};
use brain::state::{ReceiverJobToken, ReceiverObservationSet};

const AUDITED_FILES: &[&str] = &[
    "scripts/receiver_observation_bridge.py",
    "scripts/opencode_brain_plugin.js",
    "src/agent/observation.rs",
    "src/agent/observation/snapshot.rs",
    "src/agent/observation/snapshot/file.rs",
    "src/state/receiver/model.rs",
    "src/state/receiver/store/claim/next.rs",
    "src/state/receiver/store/observation.rs",
    "src/tui/app_brain/receiver/active.rs",
    "src/tui/app_brain/receiver/diagnostic.rs",
    "tests/receiver_observation_bridge.rs",
    "tests/fixtures/opencode/plugin_harness.js",
    "src/agent/observation/tests.rs",
    "src/agent/observation/file_tests.rs",
    "src/tui/app_brain/tests/receiver_durable_observation.rs",
    "src/tui/app_brain/tests/receiver_durable_observation_composed.rs",
    "src/tui/app_brain/tests/receiver_durable_observation_replacement.rs",
    "src/tui/app_brain/tests/receiver_durable_process_restart.rs",
    "src/tui/app_brain/tests/receiver_durable_producer_matrix.rs",
    "src/tui/receiver/planning_tests.rs",
];

#[test]
fn observation_sources_fixtures_and_test_names_use_only_generic_literals() {
    let root = repository_root();
    let forbidden = [
        "/Users/",
        "/home/",
        "@gmail.",
        "@icloud.",
        "@proton.",
        "sk_live_",
        "sk-proj-",
        "xoxb-",
        "ghp_",
        "AKIA",
        "corp.internal",
        "private-host",
    ];
    for relative in AUDITED_FILES {
        let source = std::fs::read_to_string(root.join(relative))
            .unwrap_or_else(|error| panic!("read {relative}: {error}"));
        for value in forbidden {
            assert!(
                !source.contains(value),
                "{relative} contains a private literal matching {value:?}"
            );
        }
        for line in source.lines().filter(|line| line.contains("://")) {
            assert!(
                line.contains(".example.test"),
                "{relative} contains a non-generic host literal: {line}"
            );
        }
    }
}

#[test]
fn observation_diagnostics_and_debug_formatting_expose_no_private_fields() {
    let token = "11111111-1111-4111-8111-111111111111";
    let instance = "22222222-2222-4222-8222-222222222222";
    let job_token = ReceiverJobToken::parse(token).expect("job token");
    assert_eq!(format!("{job_token:?}"), "ReceiverJobToken(<redacted>)");
    let session = AgentSession::new("native-private-canary").expect("native session");
    let request = AgentObservationRequest::new(
        token,
        instance,
        PathBuf::from("/opaque/private-canary.json"),
        session,
        AgentObservationCursor::launched(),
    );
    assert_eq!(
        format!("{request:?}"),
        "AgentObservationRequest(<redacted>)"
    );

    let set = ReceiverObservationSet {
        token: job_token,
        instance: instance.to_owned(),
        session_id: "native-private-canary".to_owned(),
        revision: 3,
        accepted_at_unix_ms: Some(1_000),
        progressing_at_unix_ms: Some(1_100),
        completed_at_unix_ms: Some(1_200),
        authorized_at_unix_ms: 1_300,
    };
    assert_eq!(format!("{set:?}"), "ReceiverObservationSet(<redacted>)");
    for error in [
        AgentObservationError::IdentityMismatch,
        AgentObservationError::MalformedSnapshot,
        AgentObservationError::AmbiguousLifecycle,
    ] {
        let rendered = format!("{error:?}: {error}");
        for private in [
            token,
            instance,
            "native-private-canary",
            "private-canary.json",
        ] {
            assert!(!rendered.contains(private), "error output leaked {private}");
        }
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
        "snapshot",
        "transcript",
    ] {
        assert!(
            !diagnostic.contains(private_field),
            "diagnostic formatter names private field {private_field}"
        );
    }
}

#[test]
fn normalized_snapshot_never_copies_the_submitted_prompt() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = temporary.path().join("observation.json");
    let token = "11111111-1111-4111-8111-111111111111";
    let canary = "submitted-private-canary";
    let payload = serde_json::json!({
        "hook_event_name": "UserPromptSubmit",
        "session_id": "native-session",
        "prompt": format!(
            "{canary}\n<!-- brain:receiver-job-token={token} -->"
        ),
    });
    let mut child = Command::new("python3")
        .arg(repository_root().join("scripts/receiver_observation_bridge.py"))
        .env("BRAIN_RECEIVER_JOB_TOKEN", token)
        .env("BRAIN_RECEIVER_OBSERVATION_PATH", &path)
        .env("BRAIN_INSTANCE_ID", "22222222-2222-4222-8222-222222222222")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn observation producer");
    child
        .stdin
        .take()
        .expect("producer stdin")
        .write_all(payload.to_string().as_bytes())
        .expect("write producer payload");
    let output = child.wait_with_output().expect("wait observation producer");
    assert!(output.status.success(), "producer failed: {output:?}");
    let snapshot = std::fs::read_to_string(path).expect("normalized snapshot");
    assert!(!snapshot.contains(canary));
    let value: serde_json::Value = serde_json::from_str(&snapshot).expect("snapshot JSON");
    assert_eq!(value.as_object().expect("snapshot object").len(), 10);
}

fn repository_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}
