use std::{path::PathBuf, sync::Arc};

use crate::{
    actor::{ActorContext, RequestIdentity},
    agent::{
        AccessPolicy, AgentFrontend, AgentSession, ClaudeFrontend, CodexFrontend,
        CompletionStrategy, InputSequence, LaunchRequest, SessionPlan,
    },
    users::{USERS_SCHEMA_VERSION, User, UserId, Users},
    workspace::{WorkspaceContext, WorkspaceId, WorkspaceName},
};

fn workspace() -> Arc<WorkspaceContext> {
    Arc::new(
        WorkspaceContext::new(
            std::path::Path::new("/home/tester"),
            WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").expect("valid id"),
            WorkspaceName::parse("family").expect("valid name"),
            std::path::Path::new("/workspaces/family brain"),
            "pablo",
            std::path::Path::new("/home/tester"),
        )
        .expect("context"),
    )
}

fn actor() -> ActorContext {
    let users = Users {
        schema_version: USERS_SCHEMA_VERSION,
        users: vec![User {
            id: UserId::parse("pablo").expect("valid user"),
            name: "Pablo".to_owned(),
            phones: Vec::new(),
            emails: Vec::new(),
            response_email: None,
        }],
    };
    crate::actor::resolve_actor(
        &UserId::parse("pablo").expect("valid user"),
        RequestIdentity::Local,
        &users,
    )
    .expect("actor")
}

fn request(plan: SessionPlan, prompt: Option<&str>) -> LaunchRequest {
    LaunchRequest::new(
        workspace(),
        actor(),
        plan,
        prompt.map(str::to_owned),
        AccessPolicy::default(),
    )
}

fn fresh(id: &str) -> LaunchRequest {
    request(
        SessionPlan::fresh(AgentSession::new(id).expect("session")),
        None,
    )
}

fn resume(id: &str) -> LaunchRequest {
    request(
        SessionPlan::resume(AgentSession::new(id).expect("session")),
        None,
    )
}

fn fresh_with_prompt(prompt: &str) -> LaunchRequest {
    request(
        SessionPlan::fresh(AgentSession::new("fresh-1").expect("session")),
        Some(prompt),
    )
}

#[test]
fn adapters_preserve_fresh_and_resume_command_syntax() {
    let claude = ClaudeFrontend::new(
        "claude",
        PathBuf::from("/workspaces/family brain"),
        PathBuf::from("/home/tester/.claude/projects"),
    );
    let codex = CodexFrontend::new("codex");

    assert_eq!(
        claude
            .launch_spec(&resume("sess-9"))
            .expect("Claude launch")
            .command,
        "claude --resume 'sess-9'"
    );
    assert_eq!(
        codex
            .launch_spec(&fresh_with_prompt("Start here"))
            .expect("Codex launch")
            .command,
        "codex -- 'Start here'"
    );
    assert_eq!(
        claude
            .launch_spec(&fresh("fresh-1"))
            .expect("Claude launch")
            .command,
        "claude --session-id 'fresh-1'"
    );
    assert_eq!(
        codex
            .launch_spec(&resume("sess-9"))
            .expect("Codex launch")
            .command,
        "codex resume 'sess-9'"
    );
}

#[test]
fn adapters_preserve_configured_prefix_and_prompt_quoting() {
    let claude = ClaudeFrontend::new(
        " claude --model sonnet ",
        PathBuf::from("/workspaces/family brain"),
        PathBuf::from("/home/tester/.claude/projects"),
    );
    let codex = CodexFrontend::new(" codex --model gpt-5 ");
    let prompt = Some("  don't lose this  ");

    assert_eq!(
        claude
            .launch_spec(&request(
                SessionPlan::fresh(AgentSession::new("fresh-1").expect("session")),
                prompt,
            ))
            .expect("Claude launch")
            .command,
        "claude --model sonnet --session-id 'fresh-1' -- 'don'\\''t lose this'"
    );
    assert_eq!(
        codex
            .launch_spec(&request(
                SessionPlan::resume(AgentSession::new("resume-1").expect("session")),
                prompt,
            ))
            .expect("Codex launch")
            .command,
        "codex --model gpt-5 resume 'resume-1' -- 'don'\\''t lose this'"
    );
}

#[test]
fn adapters_translate_submit_queue_and_new_session_input() {
    let claude = ClaudeFrontend::new(
        "claude",
        PathBuf::from("/workspaces/family brain"),
        PathBuf::from("/home/tester/.claude/projects"),
    );
    let codex = CodexFrontend::new("codex");

    assert_eq!(claude.submit_input(), Ok(InputSequence::bytes(b"\r")));
    assert_eq!(codex.submit_input(), Ok(InputSequence::bytes(b"\r")));
    assert_eq!(claude.queue_input(), Ok(InputSequence::bytes(b"\r")));
    assert_eq!(codex.queue_input(), Ok(InputSequence::bytes(b"\t")));
    assert_eq!(
        claude.new_session_input(),
        Ok(InputSequence::bytes(b"/new\r"))
    );
    assert_eq!(
        codex.new_session_input(),
        Ok(InputSequence::bytes(b"/new\t"))
    );
}

#[test]
fn adapters_own_completion_and_transcript_conventions() {
    let claude = ClaudeFrontend::new(
        "claude",
        PathBuf::from("/workspaces/family brain"),
        PathBuf::from("/home/tester/.claude/projects"),
    );
    let codex = CodexFrontend::new("codex");
    let session = AgentSession::new("sess-9").expect("session");

    assert_eq!(claude.completion_strategy(), Ok(CompletionStrategy::Hook));
    assert_eq!(codex.completion_strategy(), Ok(CompletionStrategy::Hook));
    assert_eq!(
        claude.transcript(&session),
        Ok(Some(PathBuf::from(
            "/home/tester/.claude/projects/-workspaces-family brain/sess-9.jsonl"
        )))
    );
    assert_eq!(codex.transcript(&session), Ok(None));
}

#[test]
fn adapters_own_session_tracking_and_response_identity() {
    let claude = ClaudeFrontend::new(
        "claude",
        PathBuf::from("/workspaces/family brain"),
        PathBuf::from("/home/tester/.claude/projects"),
    );
    let codex = CodexFrontend::new("codex");
    let session = AgentSession::new("sess-9").expect("session");

    assert_eq!(claude.response_id(&session).as_deref(), Ok("sess-9"));
    let codex_response_id = codex.response_id(&session).expect("Codex response ID");
    assert_ne!(codex_response_id, "sess-9");
    assert_eq!(
        codex.response_id(&session).as_deref(),
        Ok(codex_response_id.as_str())
    );
    assert!(uuid::Uuid::parse_str(&codex_response_id).is_ok());
    assert_eq!(claude.can_resume_response_session(), Ok(true));
    assert_eq!(codex.can_resume_response_session(), Ok(false));
}

#[test]
fn adapters_produce_complete_workspace_and_actor_launch_specs() {
    let claude = ClaudeFrontend::new(
        "claude",
        PathBuf::from("/workspaces/family brain"),
        PathBuf::from("/home/tester/.claude/projects"),
    );
    let codex = CodexFrontend::new("codex");

    let claude_spec = claude
        .launch_spec(&fresh("fresh-1"))
        .expect("Claude launch");
    assert_eq!(claude_spec.cwd, PathBuf::from("/workspaces/family brain"));
    for expected in [
        (
            "BRAIN_WORKSPACE_ID".to_owned(),
            "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b".to_owned(),
        ),
        ("BRAIN_WORKSPACE".to_owned(), "family".to_owned()),
        (
            "BRAIN_ROOT".to_owned(),
            "/workspaces/family brain".to_owned(),
        ),
        ("BRAIN_ACTOR_ID".to_owned(), "pablo".to_owned()),
        ("BRAIN_CHANNEL".to_owned(), "interactive".to_owned()),
        ("BRAIN_AGENT_KIND".to_owned(), "claude".to_owned()),
    ] {
        assert!(claude_spec.environment.contains(&expected));
    }

    let codex_spec = codex.launch_spec(&fresh("fresh-1")).expect("Codex launch");
    assert_eq!(codex_spec.cwd, PathBuf::from("/workspaces/family brain"));
    for expected in [
        (
            "BRAIN_WORKSPACE_ID".to_owned(),
            "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b".to_owned(),
        ),
        ("BRAIN_WORKSPACE".to_owned(), "family".to_owned()),
        (
            "BRAIN_ROOT".to_owned(),
            "/workspaces/family brain".to_owned(),
        ),
        ("BRAIN_ACTOR_ID".to_owned(), "pablo".to_owned()),
        ("BRAIN_CHANNEL".to_owned(), "interactive".to_owned()),
        ("BRAIN_AGENT_KIND".to_owned(), "codex".to_owned()),
    ] {
        assert!(codex_spec.environment.contains(&expected));
    }
}

#[test]
fn adapters_validate_resume_candidates_with_their_own_transcript_rules() {
    let projects = tempfile::tempdir().expect("projects dir");
    let project = projects.path().join("-workspaces-family brain");
    std::fs::create_dir(&project).expect("project dir");
    std::fs::write(project.join("valid.jsonl"), "{}\n").expect("transcript");
    let claude = ClaudeFrontend::new(
        "claude",
        PathBuf::from("/workspaces/family brain"),
        projects.path().to_path_buf(),
    );
    let codex = CodexFrontend::new("codex");

    assert_eq!(
        claude.resume_candidate_exists(&AgentSession::new("valid").expect("valid session")),
        Ok(true)
    );
    assert_eq!(
        claude.resume_candidate_exists(&AgentSession::new("missing").expect("missing session")),
        Ok(false)
    );
    assert_eq!(
        codex.resume_candidate_exists(&AgentSession::new("unvalidated").expect("Codex session")),
        Ok(false)
    );
}
