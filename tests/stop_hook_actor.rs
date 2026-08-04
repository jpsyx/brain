use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn codex_completion_uses_the_job_response_id_and_preserves_actor_context() {
    let temp = tempfile::tempdir().unwrap();
    let response_dir = temp.path().join("responses");
    let state_db = temp.path().join("state.db");
    drop(brain::state::Db::open_path(&state_db).unwrap());
    let connection = rusqlite::Connection::open(&state_db).unwrap();
    connection
        .execute(
            "INSERT INTO brain_sessions
               (agent_kind, agent_session_id, brain_instance_id, locked_pid, source,
                workspace_id, actor_id, channel, created_at, last_active_at)
             VALUES ('codex', 'codex-thread-9', 'shell-1', 42, 'test',
                     '11111111-1111-4111-8111-111111111111', 'member', 'sms', 1, 1)",
            [],
        )
        .unwrap();
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/claude_stop_hook.py");
    let mut child = Command::new("python3")
        .arg(script)
        .env("BRAIN_RESPONSE_DIR", &response_dir)
        .env("BRAIN_RESPONSE_ID", "job-7")
        .env("BRAIN_STATE_DB", &state_db)
        .env("BRAIN_WORKSPACE_ID", "11111111-1111-4111-8111-111111111111")
        .env("BRAIN_AGENT_KIND", "codex")
        .env("BRAIN_INSTANCE_ID", "shell-1")
        .env("BRAIN_ACTOR_ID", "member")
        .env("BRAIN_CHANNEL", "sms")
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(
            serde_json::json!({
                "thread_id": "codex-thread-9",
                "last_assistant_message": "Done"
            })
            .to_string()
            .as_bytes(),
        )
        .unwrap();
    drop(child.stdin.take());
    assert!(child.wait().unwrap().success());

    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(response_dir.join("job-7.json")).unwrap()).unwrap();
    assert_eq!(value["session_id"], "codex-thread-9");
    assert_eq!(value["response_id"], "job-7");
    assert_eq!(value["frontend"], "codex");
    assert_eq!(
        value["workspace_id"],
        "11111111-1111-4111-8111-111111111111"
    );
    assert_eq!(value["actor_id"], "member");
    assert_eq!(value["channel"], "sms");
    assert_eq!(value["completion_status"], "completed");
    let status: String = connection
        .query_row(
            "SELECT completion_status FROM brain_sessions
             WHERE agent_kind = 'codex' AND agent_session_id = 'codex-thread-9'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "completed");
}
