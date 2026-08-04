use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn codex_completion_uses_the_job_response_id_and_preserves_actor_context() {
    let temp = tempfile::tempdir().unwrap();
    let response_dir = temp.path().join("responses");
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/claude_stop_hook.py");
    let mut child = Command::new("python3")
        .arg(script)
        .env("BRAIN_RESPONSE_DIR", &response_dir)
        .env("BRAIN_RESPONSE_ID", "job-7")
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
    assert_eq!(value["actor_id"], "member");
    assert_eq!(value["channel"], "sms");
}
