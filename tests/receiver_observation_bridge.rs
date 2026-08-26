use std::os::unix::fs::PermissionsExt as _;

#[path = "receiver_observation_bridge/fixture_support.rs"]
mod fixture_support;
#[path = "receiver_observation_bridge/producer_boundaries.rs"]
mod producer_boundaries;

use fixture_support::*;

#[test]
fn exact_terminal_receiver_marker_writes_one_private_fixed_schema_snapshot() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = observation_path(&temporary, "nested/observation.json");
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
            "latest_progress_at_unix_ms",
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
    assert!(value["latest_progress_at_unix_ms"].is_null());
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
        let path = observation_path(&temporary, format!("{name}.json"));
        let output = run_bridge(&path, &payload);
        assert!(output.status.success(), "{name} failed: {output:?}");
        assert!(!path.exists(), "{name} produced acceptance evidence");
    }
}

#[test]
fn native_agent_id_child_submit_cannot_establish_root_acceptance() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = observation_path(&temporary, "observation.json");
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
    let path = observation_path(&temporary, "observation.json");
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
fn progress_requires_matching_acceptance_and_later_events_pulse_without_regression() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = observation_path(&temporary, "observation.json");
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
    assert_eq!(
        progressing["latest_progress_at_unix_ms"],
        progressing["progressing_at_unix_ms"]
    );

    std::thread::sleep(std::time::Duration::from_millis(2));
    assert!(
        run_bridge(&path, &progress_payload(SESSION_ID, "turn-2"))
            .status
            .success()
    );
    let pulsed = snapshot(&path);
    assert_eq!(pulsed["revision"], 3);
    assert_eq!(pulsed["turn_id"], "turn-2");
    assert_eq!(
        pulsed["progressing_at_unix_ms"],
        progressing["progressing_at_unix_ms"]
    );
    assert!(
        pulsed["latest_progress_at_unix_ms"].as_u64().unwrap()
            > progressing["latest_progress_at_unix_ms"].as_u64().unwrap()
    );
    assert!(
        run_bridge(&path, &accepted_payload(&marker))
            .status
            .success()
    );
    assert_eq!(
        snapshot(&path),
        pulsed,
        "regressed acceptance must not increment revision"
    );
}

fn frontend_submit(kind: &str, prompt: &str, turn_id: &str) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "hook_event_name": "UserPromptSubmit",
        "session_id": SESSION_ID,
        "prompt": prompt,
    });
    let field = if kind == "claude" {
        "prompt_id"
    } else {
        "turn_id"
    };
    payload[field] = serde_json::json!(turn_id);
    payload
}

fn frontend_tool(kind: &str, accepted_turn_id: &str, tool_use_id: &str) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "hook_event_name": "PostToolUse",
        "session_id": SESSION_ID,
        "tool_use_id": tool_use_id,
        "turn_id": tool_use_id,
    });
    let field = if kind == "claude" {
        "prompt_id"
    } else {
        "turn_id"
    };
    payload[field] = serde_json::json!(accepted_turn_id);
    payload
}

#[test]
fn claude_and_codex_reject_delayed_tool_events_from_a_prior_turn_after_acceptance() {
    let marker = format!("<!-- brain:receiver-job-token={JOB_TOKEN} -->");
    for kind in ["claude", "codex"] {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = observation_path(&temporary, format!("{kind}.json"));
        assert!(
            run_bridge_for_kind(
                &path,
                kind,
                &frontend_submit(kind, &marker, "receiver-turn"),
            )
            .status
            .success()
        );
        let accepted = snapshot(&path);

        assert!(
            run_bridge_for_kind(
                &path,
                kind,
                &frontend_tool(kind, "prior-unrelated-turn", "delayed-tool"),
            )
            .status
            .success()
        );
        assert_eq!(
            snapshot(&path),
            accepted,
            "{kind} accepted a delayed tool event from a prior turn"
        );

        assert!(
            run_bridge_for_kind(
                &path,
                kind,
                &frontend_tool(kind, "receiver-turn", "receiver-tool"),
            )
            .status
            .success()
        );
        let progressing = snapshot(&path);
        assert_eq!(progressing["revision"], 2, "{kind}");
        assert_eq!(progressing["turn_id"], "receiver-tool", "{kind}");
    }
}

#[test]
fn claude_and_codex_revoke_progress_after_a_later_nonmarker_root_prompt() {
    let marker = format!("<!-- brain:receiver-job-token={JOB_TOKEN} -->");
    for kind in ["claude", "codex"] {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = observation_path(&temporary, format!("{kind}.json"));
        assert!(
            run_bridge_for_kind(
                &path,
                kind,
                &frontend_submit(kind, &marker, "receiver-turn"),
            )
            .status
            .success()
        );
        assert!(
            run_bridge_for_kind(
                &path,
                kind,
                &frontend_tool(kind, "receiver-turn", "receiver-tool-1"),
            )
            .status
            .success()
        );
        let progressing = snapshot(&path);

        assert!(
            run_bridge_for_kind(
                &path,
                kind,
                &frontend_submit(kind, "ordinary follow-up", "unrelated-turn"),
            )
            .status
            .success()
        );
        assert!(
            run_bridge_for_kind(
                &path,
                kind,
                &frontend_tool(kind, "receiver-turn", "delayed-receiver-tool"),
            )
            .status
            .success()
        );
        assert!(
            run_bridge_for_kind(
                &path,
                kind,
                &frontend_tool(kind, "unrelated-turn", "unrelated-tool"),
            )
            .status
            .success()
        );
        assert_eq!(
            snapshot(&path),
            progressing,
            "{kind} retained progress authority after an unrelated prompt"
        );
    }
}

#[test]
fn concurrent_delivery_is_monotonic_and_completion_retains_every_boundary() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = observation_path(&temporary, "observation.json");
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
    let progress_revision = progressing["revision"].as_u64().unwrap();
    assert!((2..=9).contains(&progress_revision));
    assert_eq!(progressing["phase"], "progressing");
    assert_eq!(progressing["accepted_at_unix_ms"], accepted_at);
    let progressing_at = progressing["progressing_at_unix_ms"].clone();
    let latest_progress_at = progressing["latest_progress_at_unix_ms"].clone();

    let completed = serde_json::json!({
        "hook_event_name": "Stop",
        "session_id": SESSION_ID,
        "turn_id": "turn-final",
    });
    assert!(run_bridge(&path, &completed).status.success());
    let value = snapshot(&path);
    assert_eq!(value["revision"], progress_revision + 1);
    assert_eq!(value["phase"], "completed");
    assert_eq!(value["accepted_at_unix_ms"], accepted_at);
    assert_eq!(value["progressing_at_unix_ms"], progressing_at);
    assert_eq!(value["latest_progress_at_unix_ms"], latest_progress_at);
    assert!(value["completed_at_unix_ms"].as_u64().is_some());
    assert_eq!(value["turn_id"], "turn-final");

    assert!(run_bridge(&path, &completed).status.success());
    assert_eq!(
        snapshot(&path),
        value,
        "duplicate completion mutated evidence"
    );
}
