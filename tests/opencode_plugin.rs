use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const WORKSPACE_ID: &str = "11111111-1111-4111-8111-111111111111";

fn plugin_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/opencode_brain_plugin.js")
}

fn harness_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/opencode/plugin_harness.js")
}

fn available_runtimes() -> Vec<&'static str> {
    ["bun", "node"]
        .into_iter()
        .filter(|runtime| Command::new(runtime).arg("--version").output().is_ok())
        .collect()
}

fn run_harness(runtime: &str, scenario: &str, env: &[(OsString, OsString)]) -> Output {
    Command::new(runtime)
        .arg(harness_path())
        .arg(plugin_path())
        .arg(scenario)
        .envs(env.iter().cloned())
        .output()
        .unwrap_or_else(|error| panic!("run {scenario} with {runtime}: {error}"))
}

fn assert_harness_succeeds(scenario: &str) {
    let runtimes = available_runtimes();
    assert!(
        !runtimes.is_empty(),
        "the OpenCode plugin harness requires Bun or Node"
    );
    for runtime in runtimes {
        let output = run_harness(runtime, scenario, &[]);
        assert!(
            output.status.success(),
            "{scenario} failed with {runtime}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn copy_hook(root: &Path, name: &str) {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join(name);
    let destination = root.join(".claude/brain-hooks").join(name);
    std::fs::create_dir_all(destination.parent().expect("hook parent")).unwrap();
    std::fs::copy(source, destination).unwrap();
}

fn register_pending_session(state_db: &Path) {
    rusqlite::Connection::open(state_db)
        .unwrap()
        .execute(
            "INSERT INTO brain_sessions
               (agent_kind, agent_session_id, brain_instance_id, locked_pid, source,
                workspace_id, actor_id, channel, created_at, last_active_at)
             VALUES ('opencode', 'pending-opencode', 'shell-1', 42, 'test',
                     ?1, 'member', 'sms', 1, 1)",
            [WORKSPACE_ID],
        )
        .unwrap();
}

fn env_pair(name: &str, value: impl AsRef<OsStr>) -> (OsString, OsString) {
    (OsString::from(name), value.as_ref().to_os_string())
}

#[test]
fn plugin_uses_supported_sdk_calls_and_resolves_root_child_and_resumed_sessions() {
    assert_harness_succeeds("roots");
}

#[test]
fn plugin_selects_only_the_newest_completed_eligible_assistant_text() {
    assert_harness_succeeds("completion");
}

#[test]
fn plugin_logs_lookup_response_and_hook_failures_without_emitting_completion() {
    assert_harness_succeeds("errors");
}

#[test]
fn plugin_sends_payload_only_over_stdin_with_a_minimal_safe_environment() {
    assert_harness_succeeds("safety");
}

#[test]
fn repeated_idle_events_leave_one_response_artifact_through_the_real_bridge() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("family");
    let state_db = temporary.path().join("state.db");
    let response_dir = temporary.path().join("responses");
    copy_hook(&root, "agent_session_start_hook.py");
    copy_hook(&root, "agent_turn_complete_hook.py");
    drop(brain::state::Db::open_path(&state_db).unwrap());
    register_pending_session(&state_db);

    let env = vec![
        env_pair("BRAIN_WORKSPACE_ID", WORKSPACE_ID),
        env_pair("BRAIN_WORKSPACE", "family"),
        env_pair("BRAIN_ROOT", &root),
        env_pair("BRAIN_ACTOR_ID", "member"),
        env_pair("BRAIN_CHANNEL", "sms"),
        env_pair("BRAIN_AGENT_KIND", "opencode"),
        env_pair("BRAIN_INSTANCE_ID", "shell-1"),
        env_pair("BRAIN_PID", "42"),
        env_pair("BRAIN_STATE_DB", &state_db),
        env_pair("BRAIN_RESPONSE_DIR", &response_dir),
        env_pair("BRAIN_RESPONSE_ID", "job-7"),
    ];
    let runtimes = available_runtimes();
    assert!(
        !runtimes.is_empty(),
        "the OpenCode plugin harness requires Bun or Node"
    );
    let output = run_harness(runtimes[0], "repeated_idle", &env);
    assert!(
        output.status.success(),
        "real bridge scenario failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let response_files = std::fs::read_dir(&response_dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    assert_eq!(response_files, [OsString::from("job-7.json")]);
    let response: serde_json::Value =
        serde_json::from_slice(&std::fs::read(response_dir.join("job-7.json")).unwrap()).unwrap();
    assert_eq!(response["session_id"], "root-real");
    assert_eq!(response["frontend"], "opencode");
    assert_eq!(response["message"], "Completed once");

    let connection = rusqlite::Connection::open(state_db).unwrap();
    let current: (Option<i64>, String) = connection
        .query_row(
            "SELECT locked_pid, completion_status FROM brain_sessions
             WHERE agent_kind = 'opencode' AND agent_session_id = 'root-real'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let prior_lock: Option<i64> = connection
        .query_row(
            "SELECT locked_pid FROM brain_sessions
             WHERE agent_kind = 'opencode' AND agent_session_id = 'pending-opencode'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(current, (Some(42), "completed".to_owned()));
    assert_eq!(prior_lock, None);
}
