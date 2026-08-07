use super::*;

fn fresh_lifecycle(
    agent_kind: &str,
    session_id: &str,
) -> (tempfile::TempDir, PathBuf, PathBuf, String) {
    let temporary = tempfile::tempdir().unwrap();
    let state_db = temporary.path().join("state.db");
    let response_dir = temporary.path().join("responses");
    let instance = format!("{agent_kind}-shell");
    drop(brain::state::Db::open_path(&state_db).unwrap());
    register_session(&state_db, agent_kind, session_id, &instance);
    (temporary, state_db, response_dir, instance)
}

#[test]
fn normalized_completion_contract_preserves_exact_identity_for_every_frontend() {
    for agent_kind in ["claude", "codex", "opencode"] {
        let session_id = format!("{agent_kind}-session");
        let (_temporary, state_db, response_dir, instance) =
            fresh_lifecycle(agent_kind, &session_id);
        let response_id = format!("{agent_kind}-response");
        let output = run_hook(
            attributed_hook_command(
                "agent_turn_complete_hook.py",
                &state_db,
                &response_dir,
                agent_kind,
                &instance,
                &response_id,
            ),
            &serde_json::json!({
                "session_id": session_id,
                "last_assistant_message": format!("completed by {agent_kind}")
            }),
        );

        assert!(output.status.success(), "{agent_kind} failed: {output:?}");
        let response: serde_json::Value = serde_json::from_slice(
            &std::fs::read(response_dir.join(format!("{response_id}.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(response["session_id"], session_id);
        assert_eq!(response["response_id"], response_id);
        assert_eq!(response["frontend"], agent_kind);
        assert_eq!(response["workspace_id"], WORKSPACE_ID);
        assert_eq!(response["actor_id"], "member");
        assert_eq!(response["channel"], "sms");
        assert_eq!(response["completion_status"], "completed");
        assert_eq!(completion_status(&state_db, &session_id), "completed");
    }
}

#[test]
fn thread_id_is_a_codex_only_compatibility_input() {
    for agent_kind in ["claude", "opencode"] {
        let session_id = format!("{agent_kind}-session");
        let (_temporary, state_db, response_dir, instance) =
            fresh_lifecycle(agent_kind, &session_id);
        let output = run_hook(
            attributed_hook_command(
                "agent_turn_complete_hook.py",
                &state_db,
                &response_dir,
                agent_kind,
                &instance,
                "response",
            ),
            &serde_json::json!({
                "thread_id": session_id,
                "last_assistant_message": "must be ignored"
            }),
        );
        assert!(output.status.success());
        assert_eq!(completion_status(&state_db, &session_id), "active");
        assert!(!response_dir.join("response.json").exists());
    }
}

#[test]
fn transcript_fallback_is_claude_only() {
    for agent_kind in ["codex", "opencode"] {
        let session_id = format!("{agent_kind}-session");
        let (temporary, state_db, response_dir, instance) =
            fresh_lifecycle(agent_kind, &session_id);
        let transcript = temporary.path().join("transcript.jsonl");
        std::fs::write(
            &transcript,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Claude-only fallback"}]}}"#,
        )
        .unwrap();
        let output = run_hook(
            attributed_hook_command(
                "agent_turn_complete_hook.py",
                &state_db,
                &response_dir,
                agent_kind,
                &instance,
                "response",
            ),
            &serde_json::json!({
                "session_id": session_id,
                "transcript_path": transcript
            }),
        );
        assert!(output.status.success());
        assert_eq!(completion_status(&state_db, &session_id), "active");
        assert!(!response_dir.join("response.json").exists());
    }
}

#[test]
fn duplicate_and_mismatched_completion_events_are_noops() {
    let session_id = "protected-session";
    let (_temporary, state_db, response_dir, instance) = fresh_lifecycle("opencode", session_id);
    let target = response_dir.join("response.json");
    let complete = || {
        run_hook(
            attributed_hook_command(
                "agent_turn_complete_hook.py",
                &state_db,
                &response_dir,
                "opencode",
                &instance,
                "response",
            ),
            &serde_json::json!({
                "session_id": session_id,
                "last_assistant_message": "first"
            }),
        )
    };
    assert!(complete().status.success());
    std::fs::write(&target, b"preserve duplicate target").unwrap();
    assert!(complete().status.success());
    assert_eq!(
        std::fs::read(&target).unwrap(),
        b"preserve duplicate target"
    );

    for (agent_kind, instance) in [("claude", instance.as_str()), ("opencode", "other-shell")] {
        if target.exists() {
            std::fs::remove_file(&target).unwrap();
        }
        let output = run_hook(
            attributed_hook_command(
                "agent_turn_complete_hook.py",
                &state_db,
                &response_dir,
                agent_kind,
                instance,
                "response",
            ),
            &serde_json::json!({
                "session_id": session_id,
                "last_assistant_message": "wrong identity"
            }),
        );
        assert!(output.status.success());
        assert!(!target.exists(), "{agent_kind}/{instance} published");
    }
}

#[test]
fn wrong_workspace_actor_and_channel_cannot_complete_a_registered_lineage() {
    for (name, value) in [
        ("BRAIN_WORKSPACE_ID", "22222222-2222-4222-8222-222222222222"),
        ("BRAIN_ACTOR_ID", "other-actor"),
        ("BRAIN_CHANNEL", "email"),
    ] {
        let session_id = "protected-session";
        let (_temporary, state_db, response_dir, instance) =
            fresh_lifecycle("opencode", session_id);
        let mut command = attributed_hook_command(
            "agent_turn_complete_hook.py",
            &state_db,
            &response_dir,
            "opencode",
            &instance,
            "response",
        );
        command.env(name, value);

        let output = run_hook(
            command,
            &serde_json::json!({
                "session_id": session_id,
                "last_assistant_message": "must not publish"
            }),
        );

        assert!(output.status.success(), "{name} failed: {output:?}");
        assert_eq!(completion_status(&state_db, session_id), "active");
        assert!(!response_dir.join("response.json").exists());
    }
}

#[test]
fn child_completion_payload_is_a_noop_for_every_frontend() {
    for agent_kind in ["claude", "codex", "opencode"] {
        let session_id = format!("{agent_kind}-child");
        let (_temporary, state_db, response_dir, instance) =
            fresh_lifecycle(agent_kind, &session_id);
        let output = run_hook(
            attributed_hook_command(
                "agent_turn_complete_hook.py",
                &state_db,
                &response_dir,
                agent_kind,
                &instance,
                "response",
            ),
            &serde_json::json!({
                "session_id": session_id,
                "parent_session_id": "root-session",
                "last_assistant_message": "must not publish"
            }),
        );

        assert!(output.status.success(), "{agent_kind} failed: {output:?}");
        assert_eq!(completion_status(&state_db, &session_id), "active");
        assert!(!response_dir.join("response.json").exists());
    }
}

#[test]
fn sqlite_commit_failure_restores_exact_prior_artifact_and_active_status() {
    let session_id = "commit-failure";
    let (_temporary, state_db, response_dir, instance) = fresh_lifecycle("opencode", session_id);
    std::fs::create_dir_all(&response_dir).unwrap();
    let target = response_dir.join("response.json");
    std::fs::write(&target, b"prior artifact bytes").unwrap();
    let connection = rusqlite::Connection::open(&state_db).unwrap();
    connection
        .execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE completion_parent (id INTEGER PRIMARY KEY);
             CREATE TABLE completion_child (
               id INTEGER REFERENCES completion_parent(id) DEFERRABLE INITIALLY DEFERRED
             );
             CREATE TRIGGER reject_completion_commit
             AFTER UPDATE OF completion_status ON brain_sessions
             WHEN NEW.agent_session_id = 'commit-failure'
             BEGIN
               INSERT INTO completion_child(id) VALUES (99);
             END;",
        )
        .unwrap();
    drop(connection);

    let output = run_hook(
        attributed_hook_command(
            "agent_turn_complete_hook.py",
            &state_db,
            &response_dir,
            "opencode",
            &instance,
            "response",
        ),
        &serde_json::json!({
            "session_id": session_id,
            "last_assistant_message": "must roll back"
        }),
    );

    assert!(output.status.success(), "hook leaked failure: {output:?}");
    assert_eq!(std::fs::read(target).unwrap(), b"prior artifact bytes");
    assert_eq!(completion_status(&state_db, session_id), "active");
}
