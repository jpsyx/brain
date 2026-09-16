//! The real pi lifecycle bridge under Node, against capture hooks and the
//! actual generic bridges.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const WORKSPACE_ID: &str = "11111111-1111-4111-8111-111111111111";

fn extension_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/pi_brain_extension.ts")
}

fn harness_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pi/extension_harness.js")
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
        .arg(extension_path())
        .arg(scenario)
        .envs(env.iter().cloned())
        .output()
        .unwrap_or_else(|error| panic!("run {scenario} with {runtime}: {error}"))
}

fn assert_harness_succeeds(scenario: &str) {
    let runtimes = available_runtimes();
    assert!(
        !runtimes.is_empty(),
        "the pi extension harness requires Bun or Node"
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

#[test]
fn extension_registers_only_the_events_the_bridges_need() {
    assert_harness_succeeds("registration");
}

#[test]
fn session_start_reports_pi_reason_as_the_bridge_source_and_skips_an_ephemeral_session() {
    assert_harness_succeeds("session_start");
}

#[test]
fn a_settled_turn_publishes_its_last_assistant_text_exactly_once() {
    assert_harness_succeeds("completion");
}

#[test]
fn an_errored_or_aborted_turn_publishes_no_answer() {
    assert_harness_succeeds("errored_turn");
}

#[test]
fn receiver_authority_comes_only_from_the_exact_marker_and_bounds_its_tool_events() {
    assert_harness_succeeds("observations");
}

#[test]
fn hooks_receive_payloads_on_stdin_with_a_minimal_environment() {
    assert_harness_succeeds("safety");
}

#[test]
fn a_direct_pi_session_without_a_selected_workspace_is_a_clean_no_op() {
    assert_harness_succeeds("no_root");
}

fn copy_hook(root: &Path, name: &str) {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join(name);
    let destination = root.join(".brain/hooks").join(name);
    std::fs::create_dir_all(destination.parent().expect("hook parent")).unwrap();
    std::fs::copy(source, destination).unwrap();
}

fn env_pair(name: &str, value: impl AsRef<OsStr>) -> (OsString, OsString) {
    (OsString::from(name), value.as_ref().to_os_string())
}

/// The extension, the real Python bridges, and the real state database: a
/// settled turn becomes exactly one response artifact, and settling again
/// leaves it alone.
#[test]
fn a_settled_turn_produces_one_response_artifact_through_the_real_bridges() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("family");
    let state_db = temporary.path().join("state.db");
    let response_dir = temporary.path().join("responses");
    copy_hook(&root, "agent_session_start_hook.py");
    copy_hook(&root, "agent_session_stop_hook.py");
    drop(brain::state::Db::open_path(&state_db).unwrap());
    rusqlite::Connection::open(&state_db)
        .unwrap()
        .execute(
            "INSERT INTO brain_sessions
               (agent_kind, agent_session_id, brain_instance_id, locked_pid, source,
                workspace_id, actor_id, channel, created_at, last_active_at)
             VALUES ('pi', 'pending-pi', 'shell-1', 42, 'test',
                     ?1, 'member', 'sms', 1, 1)",
            [WORKSPACE_ID],
        )
        .unwrap();

    let env = vec![
        env_pair("BRAIN_WORKSPACE_ID", WORKSPACE_ID),
        env_pair("BRAIN_WORKSPACE", "family"),
        env_pair("BRAIN_ROOT", &root),
        env_pair("BRAIN_ACTOR_ID", "member"),
        env_pair("BRAIN_CHANNEL", "sms"),
        env_pair("BRAIN_AGENT_KIND", "pi"),
        env_pair("BRAIN_INSTANCE_ID", "shell-1"),
        env_pair("BRAIN_PID", "42"),
        env_pair("BRAIN_STATE_DB", &state_db),
        env_pair("BRAIN_RESPONSE_DIR", &response_dir),
        env_pair("BRAIN_RESPONSE_ID", "job-7"),
    ];
    let runtimes = available_runtimes();
    assert!(
        !runtimes.is_empty(),
        "the pi extension harness requires Bun or Node"
    );

    let output = run_harness(runtimes[0], "real_bridges", &env);

    assert!(
        output.status.success(),
        "real bridge scenario failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let artifacts = std::fs::read_dir(&response_dir)
        .expect("response directory")
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "json"))
        .collect::<Vec<_>>();
    assert_eq!(artifacts.len(), 1, "{artifacts:?}");
    let artifact: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&artifacts[0]).unwrap()).unwrap();
    assert_eq!(artifact["frontend"], "pi");
    assert_eq!(artifact["message"], "settled answer");
    assert_eq!(artifact["session_id"], "rotated-pi-session");
    assert_eq!(artifact["completion_status"], "completed");
}
