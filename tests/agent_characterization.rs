//! Behavior-preserving characterization of the current agent frontends.
//!
//! These tests intentionally exercise the existing session builders and hook
//! scripts. Later adapter extraction copies these externally visible outcomes
//! rather than treating either frontend's syntax as a new design decision.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use brain::actor::{RequestIdentity, resolve_actor};
use brain::session::{AgentKind, Plan, build_llm_command, env_for_triage};
use brain::state::Db;
use brain::users::{USERS_SCHEMA_VERSION, User, UserId, Users};
use brain::workspace::{WorkspaceContext, WorkspaceId, WorkspaceName};

fn workspace() -> WorkspaceContext {
    WorkspaceContext::new(
        Path::new("/home/tester"),
        WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").expect("valid workspace id"),
        WorkspaceName::parse("family").expect("valid workspace name"),
        Path::new("/workspaces/family"),
        "pablo",
        Path::new("/home/tester"),
    )
    .expect("workspace context")
}

fn actor() -> brain::actor::ActorContext {
    let users = Users {
        schema_version: USERS_SCHEMA_VERSION,
        users: vec![User {
            id: UserId::parse("pablo").expect("valid user id"),
            name: "Pablo".to_owned(),
            phones: Vec::new(),
            emails: Vec::new(),
            response_email: None,
        }],
    };
    resolve_actor(
        &UserId::parse("pablo").expect("valid user id"),
        RequestIdentity::Local,
        &users,
    )
    .expect("local actor")
}

fn hook_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join(name)
}

fn run_hook(mut command: Command, input: &serde_json::Value) -> std::process::Output {
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    let mut child = command.spawn().expect("spawn hook");
    child
        .stdin
        .as_mut()
        .expect("hook stdin")
        .write_all(input.to_string().as_bytes())
        .expect("write hook input");
    drop(child.stdin.take());
    child.wait_with_output().expect("wait for hook")
}

fn run_session_start_hook(db_path: &Path, session_id: &str) -> std::process::Output {
    let mut command = Command::new("python3");
    command.arg(hook_path("claude_session_start_hook.py"));
    command.env("BRAIN_WORKSPACE_ID", "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b");
    command.env("BRAIN_WORKSPACE", "family");
    command.env("BRAIN_ROOT", "/workspaces/family");
    command.env("BRAIN_ACTOR_ID", "pablo");
    command.env("BRAIN_CHANNEL", "interactive");
    command.env("BRAIN_AGENT_KIND", "claude");
    command.env("BRAIN_INSTANCE_ID", "shell-1");
    command.env("BRAIN_PID", "4242");
    command.env("BRAIN_STATE_DB", db_path);
    let input = serde_json::json!({
        "session_id": session_id,
        "source": "startup",
        "hook_event_name": "SessionStart",
    });
    run_hook(command, &input)
}

#[test]
fn fresh_and_resumed_launches_preserve_cwd_prefix_and_initial_prompt_quoting() {
    let root = Path::new("/workspaces/family brain");
    let prompt = Some("  don't lose this  ");
    let cases = [
        (
            AgentKind::Claude,
            " claude --model sonnet ",
            Plan::Fresh("fresh-1".to_owned()),
            "cd '/workspaces/family brain' && claude --model sonnet --session-id 'fresh-1' 'don'\\''t lose this'",
        ),
        (
            AgentKind::Claude,
            " claude --model sonnet ",
            Plan::Resume("resume-1".to_owned()),
            "cd '/workspaces/family brain' && claude --model sonnet --resume 'resume-1' 'don'\\''t lose this'",
        ),
        (
            AgentKind::Codex,
            " codex --model gpt-5 ",
            Plan::Fresh("fresh-1".to_owned()),
            "cd '/workspaces/family brain' && codex --model gpt-5 'don'\\''t lose this'",
        ),
        (
            AgentKind::Codex,
            " codex --model gpt-5 ",
            Plan::Resume("resume-1".to_owned()),
            "cd '/workspaces/family brain' && codex --model gpt-5 resume 'resume-1' 'don'\\''t lose this'",
        ),
    ];

    for (agent, configured_command, plan, expected) in cases {
        assert_eq!(
            build_llm_command(root, agent, configured_command, &plan, prompt),
            expected
        );
    }
}

#[test]
fn triage_launch_environment_is_untracked_for_both_frontends() {
    for (agent, expected_kind) in [(AgentKind::Claude, "claude"), (AgentKind::Codex, "codex")] {
        let env = env_for_triage(
            &workspace(),
            &actor(),
            agent,
            "http://127.0.0.1:8787/triage/done",
            "triage-token",
        );
        let keys: Vec<&str> = env.iter().map(|(key, _)| key).map(String::as_str).collect();

        assert!(env.contains(&("BRAIN_AGENT_KIND".to_owned(), expected_kind.to_owned())));
        assert!(env.contains(&("BRAIN_TRIAGE_TOKEN".to_owned(), "triage-token".to_owned())));
        assert!(!keys.contains(&"BRAIN_INSTANCE_ID"));
        assert!(!keys.contains(&"BRAIN_STATE_DB"));
    }
}

#[test]
fn new_session_hook_releases_the_prior_session_for_resume() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let db_path = temporary.path().join("state.db");
    drop(Db::open_path(&db_path).expect("state db"));

    let first = run_session_start_hook(&db_path, "session-before-new");
    assert!(
        first.status.success(),
        "first hook failed: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let second = run_session_start_hook(&db_path, "session-after-new");
    assert!(
        second.status.success(),
        "second hook failed: {}",
        String::from_utf8_lossy(&second.stderr)
    );

    let connection = rusqlite::Connection::open(db_path).expect("read state db");
    let sessions = connection
        .prepare(
            "SELECT agent_session_id, locked_pid
             FROM brain_sessions
             ORDER BY agent_session_id",
        )
        .expect("session query")
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?))
        })
        .expect("run session query")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("collect sessions");

    assert_eq!(
        sessions,
        vec![
            ("session-after-new".to_owned(), Some(4242)),
            ("session-before-new".to_owned(), None),
        ]
    );
}

#[test]
fn completion_hook_keeps_job_identity_and_actor_context_for_each_frontend() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let response_dir = temporary.path().join("responses");
    let cases = [
        ("session_id", "claude-session-9"),
        ("thread_id", "codex-thread-9"),
    ];

    for (field, session_id) in cases {
        let mut command = Command::new("python3");
        command.arg(hook_path("claude_stop_hook.py"));
        command.env("BRAIN_RESPONSE_DIR", &response_dir);
        command.env("BRAIN_RESPONSE_ID", format!("job-{field}"));
        command.env("BRAIN_ACTOR_ID", "pablo");
        command.env("BRAIN_CHANNEL", "interactive");
        let input = serde_json::json!({field: session_id, "last_assistant_message": "Done"});
        let output = run_hook(command, &input);
        assert!(
            output.status.success(),
            "completion hook failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let value: serde_json::Value = serde_json::from_slice(
            &std::fs::read(response_dir.join(format!("job-{field}.json")))
                .expect("completion artifact"),
        )
        .expect("completion JSON");
        assert_eq!(value["session_id"], session_id);
        assert_eq!(value["response_id"], format!("job-{field}"));
        assert_eq!(value["actor_id"], "pablo");
        assert_eq!(value["channel"], "interactive");
        assert_eq!(value["message"], "Done");
    }
}
