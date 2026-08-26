use super::*;

#[test]
fn receiver_completion_observation_settles_before_session_and_artifact_publication() {
    use std::io::{Read as _, Write as _};

    const PAUSE_OBSERVATION: &str = r#"
import importlib.util
import os

spec = importlib.util.spec_from_file_location("brain_stop_hook", os.environ["BRAIN_TEST_HOOK"])
hook = importlib.util.module_from_spec(spec)
spec.loader.exec_module(hook)

def paused_observation(session_id, turn_id):
    with open(os.environ["BRAIN_TEST_READY"], "w", encoding="utf-8") as ready:
        ready.write("ready")
    with open(os.environ["BRAIN_TEST_GO"], "r", encoding="utf-8") as gate:
        gate.read(1)
    return True

hook.publish_completed_observation = paused_observation
hook.main()
"#;

    let temporary = tempfile::tempdir().unwrap();
    let response_dir = temporary.path().join("responses");
    let state_db = temporary.path().join("state.db");
    let ready = temporary.path().join("observation-ready.fifo");
    let go = temporary.path().join("observation-go.fifo");
    nix::unistd::mkfifo(
        &ready,
        nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
    )
    .unwrap();
    nix::unistd::mkfifo(
        &go,
        nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
    )
    .unwrap();
    drop(brain::state::Db::open_path(&state_db).unwrap());
    register_session(
        &state_db,
        "claude",
        "receiver-stop-session",
        "receiver-stop-instance",
    );
    let target = response_dir.join("receiver-stop-response.json");
    let observation = temporary.path().join("receiver-stop-observation.json");
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
    let ready_path = ready.clone();
    let ready_reader = std::thread::spawn(move || {
        let mut signal = String::new();
        std::fs::File::open(ready_path)
            .unwrap()
            .read_to_string(&mut signal)
            .unwrap();
        ready_tx.send(signal).unwrap();
    });
    let mut command = attributed_command(
        Command::new("python3"),
        &state_db,
        &response_dir,
        "claude",
        "receiver-stop-instance",
        "receiver-stop-response",
    );
    command.args(["-c", PAUSE_OBSERVATION]);
    command
        .env("BRAIN_TEST_HOOK", hook_script("agent_session_stop_hook.py"))
        .env("BRAIN_TEST_READY", &ready)
        .env("BRAIN_TEST_GO", &go)
        .env(
            "BRAIN_RECEIVER_JOB_TOKEN",
            "33333333-3333-4333-8333-333333333333",
        )
        .env("BRAIN_RECEIVER_OBSERVATION_PATH", &observation);
    let child = spawn_hook(
        command,
        &serde_json::json!({
            "session_id": "receiver-stop-session",
            "last_assistant_message": "receiver response",
        }),
    );

    assert_eq!(
        ready_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        "ready"
    );
    ready_reader.join().unwrap();
    let status_while_observation_is_paused = completion_status(&state_db, "receiver-stop-session");
    let artifact_visible_while_observation_is_paused = target.exists();
    std::fs::File::options()
        .write(true)
        .open(&go)
        .unwrap()
        .write_all(b"g")
        .unwrap();
    let output = child.wait_with_output().unwrap();

    assert!(output.status.success(), "stop hook failed: {output:?}");
    assert_eq!(status_while_observation_is_paused, "active");
    assert!(!artifact_visible_while_observation_is_paused);
    assert_eq!(
        completion_status(&state_db, "receiver-stop-session"),
        "completed"
    );
    assert!(target.exists());
}
