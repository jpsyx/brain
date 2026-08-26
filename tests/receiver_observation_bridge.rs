use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const JOB_TOKEN: &str = "11111111-1111-4111-8111-111111111111";
const INSTANCE_ID: &str = "22222222-2222-4222-8222-222222222222";
const SESSION_ID: &str = "receiver-session-1";

fn bridge_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("receiver_observation_bridge.py")
}

fn run_bridge(snapshot: &Path, payload: &serde_json::Value) -> Output {
    let mut command = Command::new("python3");
    command
        .arg(bridge_path())
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

fn accepted_payload(prompt: &str) -> serde_json::Value {
    serde_json::json!({
        "hook_event_name": "UserPromptSubmit",
        "session_id": SESSION_ID,
        "prompt": prompt,
    })
}

fn snapshot(path: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(path).expect("observation snapshot"))
        .expect("snapshot JSON")
}

fn progress_payload(session_id: &str, turn_id: &str) -> serde_json::Value {
    serde_json::json!({
        "hook_event_name": "PostToolUse",
        "session_id": session_id,
        "turn_id": turn_id,
    })
}

fn spawn_bridge(snapshot: &Path, payload: &serde_json::Value) -> std::process::Child {
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

#[test]
fn exact_terminal_receiver_marker_writes_one_private_fixed_schema_snapshot() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = temporary.path().join("nested/observation.json");
    let marker = format!("<!-- brain:receiver-job-token={JOB_TOKEN} -->");
    let output = run_bridge(&path, &accepted_payload(&format!("synthetic\n{marker}")));

    assert!(
        output.status.success(),
        "bridge failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    let value = snapshot(&path);
    assert_eq!(
        value
            .as_object()
            .expect("snapshot object")
            .keys()
            .collect::<Vec<_>>(),
        [
            "accepted_at_unix_ms",
            "completed_at_unix_ms",
            "instance_id",
            "job_token",
            "phase",
            "progressing_at_unix_ms",
            "revision",
            "session_id",
            "turn_id",
            "version",
        ]
    );
    assert_eq!(value["version"], 1);
    assert_eq!(value["revision"], 1);
    assert_eq!(value["phase"], "accepted");
    assert_eq!(value["job_token"], JOB_TOKEN);
    assert_eq!(value["instance_id"], INSTANCE_ID);
    assert_eq!(value["session_id"], SESSION_ID);
    assert!(value["turn_id"].is_null());
    assert!(value["accepted_at_unix_ms"].as_u64().is_some());
    assert!(value["progressing_at_unix_ms"].is_null());
    assert!(value["completed_at_unix_ms"].is_null());
    assert!(std::fs::metadata(&path).unwrap().len() <= 4096);
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(path.with_extension("json.lock"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn acceptance_rejects_nonterminal_mismatched_and_child_markers_without_artifacts() {
    let marker = format!("<!-- brain:receiver-job-token={JOB_TOKEN} -->");
    for (name, payload) in [
        (
            "nonterminal",
            accepted_payload(&format!("{marker}\nsynthetic trailing line")),
        ),
        (
            "substring",
            accepted_payload(&format!("synthetic {marker}")),
        ),
        (
            "wrong token",
            accepted_payload(
                "<!-- brain:receiver-job-token=22222222-2222-4222-8222-222222222222 -->",
            ),
        ),
        (
            "child",
            serde_json::json!({
                "hook_event_name": "UserPromptSubmit",
                "session_id": SESSION_ID,
                "parent_session_id": "parent-session",
                "prompt": marker,
            }),
        ),
    ] {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = temporary.path().join(format!("{name}.json"));
        let output = run_bridge(&path, &payload);
        assert!(output.status.success(), "{name} failed: {output:?}");
        assert!(!path.exists(), "{name} produced acceptance evidence");
    }
}

#[test]
fn native_agent_id_child_submit_cannot_establish_root_acceptance() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = temporary.path().join("observation.json");
    let marker = format!("<!-- brain:receiver-job-token={JOB_TOKEN} -->");
    let payload = serde_json::json!({
        "hook_event_name": "UserPromptSubmit",
        "session_id": SESSION_ID,
        "agent_id": "child-agent-1",
        "prompt": marker,
    });

    assert!(run_bridge(&path, &payload).status.success());
    assert!(
        !path.exists(),
        "a native child submit must not establish root acceptance"
    );
}

#[test]
fn native_agent_id_child_post_tool_cannot_advance_root_progress() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = temporary.path().join("observation.json");
    let marker = format!("<!-- brain:receiver-job-token={JOB_TOKEN} -->");
    assert!(
        run_bridge(&path, &accepted_payload(&marker))
            .status
            .success()
    );
    let accepted = snapshot(&path);
    let payload = serde_json::json!({
        "hook_event_name": "PostToolUse",
        "session_id": SESSION_ID,
        "agent_id": "child-agent-1",
        "turn_id": "child-turn",
    });

    assert!(run_bridge(&path, &payload).status.success());
    assert_eq!(
        snapshot(&path),
        accepted,
        "a native child tool event must not advance root progress"
    );
}

#[test]
fn progress_requires_matching_acceptance_and_duplicate_or_regressed_events_are_noops() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = temporary.path().join("observation.json");
    let marker = format!("<!-- brain:receiver-job-token={JOB_TOKEN} -->");

    assert!(
        run_bridge(&path, &progress_payload(SESSION_ID, "turn-before"))
            .status
            .success()
    );
    assert!(!path.exists(), "progress cannot invent acceptance");
    assert!(
        run_bridge(&path, &accepted_payload(&marker))
            .status
            .success()
    );
    let accepted = snapshot(&path);
    assert!(
        run_bridge(&path, &progress_payload("other-session", "turn-wrong"))
            .status
            .success()
    );
    assert_eq!(
        snapshot(&path),
        accepted,
        "wrong-session progress mutated evidence"
    );

    assert!(
        run_bridge(&path, &progress_payload(SESSION_ID, "turn-1"))
            .status
            .success()
    );
    let progressing = snapshot(&path);
    assert_eq!(progressing["revision"], 2);
    assert_eq!(progressing["phase"], "progressing");
    assert_eq!(progressing["turn_id"], "turn-1");
    assert_eq!(
        progressing["accepted_at_unix_ms"], accepted["accepted_at_unix_ms"],
        "progress must retain the accepted boundary"
    );
    assert!(progressing["progressing_at_unix_ms"].as_u64().is_some());

    assert!(
        run_bridge(&path, &progress_payload(SESSION_ID, "turn-duplicate"))
            .status
            .success()
    );
    assert!(
        run_bridge(&path, &accepted_payload(&marker))
            .status
            .success()
    );
    assert_eq!(
        snapshot(&path),
        progressing,
        "duplicate progress and regressed acceptance must not increment revision"
    );
}

#[test]
fn concurrent_delivery_is_monotonic_and_completion_retains_every_boundary() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = temporary.path().join("observation.json");
    let marker = format!("<!-- brain:receiver-job-token={JOB_TOKEN} -->");
    assert!(
        run_bridge(&path, &accepted_payload(&marker))
            .status
            .success()
    );
    let accepted_at = snapshot(&path)["accepted_at_unix_ms"].clone();

    let children = (0..8)
        .map(|index| {
            spawn_bridge(
                &path,
                &progress_payload(SESSION_ID, &format!("turn-{index}")),
            )
        })
        .collect::<Vec<_>>();
    for child in children {
        let output = child.wait_with_output().expect("wait concurrent bridge");
        assert!(
            output.status.success(),
            "concurrent bridge failed: {output:?}"
        );
    }
    let progressing = snapshot(&path);
    assert_eq!(progressing["revision"], 2);
    assert_eq!(progressing["phase"], "progressing");
    assert_eq!(progressing["accepted_at_unix_ms"], accepted_at);
    let progressing_at = progressing["progressing_at_unix_ms"].clone();

    let completed = serde_json::json!({
        "hook_event_name": "Stop",
        "session_id": SESSION_ID,
        "turn_id": "turn-final",
    });
    assert!(run_bridge(&path, &completed).status.success());
    let value = snapshot(&path);
    assert_eq!(value["revision"], 3);
    assert_eq!(value["phase"], "completed");
    assert_eq!(value["accepted_at_unix_ms"], accepted_at);
    assert_eq!(value["progressing_at_unix_ms"], progressing_at);
    assert!(value["completed_at_unix_ms"].as_u64().is_some());
    assert_eq!(value["turn_id"], "turn-final");

    assert!(run_bridge(&path, &completed).status.success());
    assert_eq!(
        snapshot(&path),
        value,
        "duplicate completion mutated evidence"
    );
}

#[test]
fn completion_first_writes_revision_one_with_null_intermediate_boundaries() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = temporary.path().join("observation.json");
    let completed = serde_json::json!({
        "hook_event_name": "Stop",
        "session_id": SESSION_ID,
        "turn_id": "turn-final",
    });

    assert!(run_bridge(&path, &completed).status.success());
    let value = snapshot(&path);
    assert_eq!(value["revision"], 1);
    assert_eq!(value["phase"], "completed");
    assert_eq!(value["job_token"], JOB_TOKEN);
    assert_eq!(value["instance_id"], INSTANCE_ID);
    assert_eq!(value["session_id"], SESSION_ID);
    assert_eq!(value["turn_id"], "turn-final");
    assert!(value["accepted_at_unix_ms"].is_null());
    assert!(value["progressing_at_unix_ms"].is_null());
    assert!(value["completed_at_unix_ms"].as_u64().is_some());
}

#[test]
fn revision_saturation_preserves_the_last_valid_snapshot_for_later_events() {
    let cases = [
        (
            serde_json::json!({
                "version": 1,
                "revision": i64::MAX,
                "phase": "accepted",
                "job_token": JOB_TOKEN,
                "instance_id": INSTANCE_ID,
                "session_id": SESSION_ID,
                "turn_id": null,
                "accepted_at_unix_ms": 1_000,
                "progressing_at_unix_ms": null,
                "completed_at_unix_ms": null,
            }),
            progress_payload(SESSION_ID, "turn-after-saturation"),
        ),
        (
            serde_json::json!({
                "version": 1,
                "revision": i64::MAX,
                "phase": "progressing",
                "job_token": JOB_TOKEN,
                "instance_id": INSTANCE_ID,
                "session_id": SESSION_ID,
                "turn_id": "turn-before-saturation",
                "accepted_at_unix_ms": 1_000,
                "progressing_at_unix_ms": 1_100,
                "completed_at_unix_ms": null,
            }),
            serde_json::json!({
                "hook_event_name": "Stop",
                "session_id": SESSION_ID,
                "turn_id": "turn-after-saturation",
            }),
        ),
    ];

    for (index, (before, event)) in cases.into_iter().enumerate() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = temporary.path().join(format!("saturated-{index}.json"));
        std::fs::write(&path, before.to_string()).expect("saturated snapshot");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .expect("owner-only saturated snapshot");

        let output = run_bridge(&path, &event);

        assert!(output.status.success(), "bridge failed: {output:?}");
        assert_eq!(
            snapshot(&path),
            before,
            "case {index} replaced the last representable revision"
        );
    }
}
