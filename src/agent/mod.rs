//! Frontend-neutral control of interactive coding agents.
//!
//! The facade accepts semantic operations such as typing, submitting, and
//! queueing work. Frontends translate those operations into their own launch,
//! input, hook, and transcript conventions; transports own the PTY details.

mod claude;
mod codex;
mod controller;
pub mod frontend;
pub mod hooks;
pub mod input;
pub mod session;

use std::{
    error::Error,
    fmt::{Display, Formatter},
};

pub use crate::access::AccessPolicy;
pub use claude::ClaudeFrontend;
pub use codex::CodexFrontend;
pub use controller::{AgentController, AgentTransport};
pub use frontend::{AgentFrontend, LaunchRequest, LaunchSpec};
pub use hooks::HookMetadata;
pub use input::InputSequence;
pub use session::{
    AgentKind, AgentSession, CompletionStatus, CompletionStrategy, SessionPlan, SessionScope,
    SessionStore,
};

pub(crate) use claude::DEFAULT_COMMAND as DEFAULT_CLAUDE_COMMAND;
pub(crate) use claude::project_dir_name as claude_project_dir_name;
pub(crate) use codex::DEFAULT_COMMAND as DEFAULT_CODEX_COMMAND;

pub(crate) fn configured_command(
    command: &crate::workspace::CommandContext,
    kind: AgentKind,
) -> String {
    match kind {
        AgentKind::Claude => crate::env::resolve_one(command, "claude_cmd")
            .unwrap_or_else(|| DEFAULT_CLAUDE_COMMAND.to_owned()),
        AgentKind::Codex => crate::env::resolve_one(command, "codex_cmd")
            .unwrap_or_else(|| DEFAULT_CODEX_COMMAND.to_owned()),
    }
}

pub(crate) fn configured_frontend(
    command: &crate::workspace::CommandContext,
    kind: AgentKind,
) -> Box<dyn AgentFrontend> {
    let configured = configured_command(command, kind);
    match kind {
        AgentKind::Claude => {
            let workspace_root = command.workspace.root().to_path_buf();
            if let Some(home) = std::env::var_os("HOME") {
                Box::new(ClaudeFrontend::new(
                    configured,
                    workspace_root,
                    std::path::PathBuf::from(home)
                        .join(".claude")
                        .join("projects"),
                ))
            } else {
                Box::new(ClaudeFrontend::without_projects_dir(
                    configured,
                    workspace_root,
                ))
            }
        }
        AgentKind::Codex => Box::new(CodexFrontend::new(configured)),
    }
}

pub(crate) fn build_command(
    kind: AgentKind,
    configured_command: &str,
    plan: &SessionPlan,
    prompt: Option<&str>,
) -> String {
    match kind {
        AgentKind::Claude => ClaudeFrontend::command_for(configured_command, plan, prompt),
        AgentKind::Codex => CodexFrontend::command_for(configured_command, plan, prompt),
    }
}

/// A facade, frontend, or transport operation could not complete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentError {
    /// A controller operation needs non-blank semantic text.
    EmptyInput,
    /// A frontend session identifier was blank.
    EmptySessionId,
    /// A launch request did not match its controller's immutable context.
    ContextMismatch,
    /// A frontend rejected an otherwise valid facade request.
    Frontend(String),
    /// A transport could not start or communicate with its child process.
    Transport(String),
    /// A selected frontend is known but intentionally unavailable.
    UnsupportedFrontend(AgentKind),
}

impl Display for AgentError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyInput => formatter.write_str("agent input cannot be blank"),
            Self::EmptySessionId => formatter.write_str("agent session id cannot be blank"),
            Self::ContextMismatch => {
                formatter.write_str("launch request does not match controller context")
            }
            Self::Frontend(message) => write!(formatter, "frontend error: {message}"),
            Self::Transport(message) => write!(formatter, "transport error: {message}"),
            Self::UnsupportedFrontend(kind) => {
                write!(formatter, "{} is not supported", kind.label())
            }
        }
    }
}

impl Error for AgentError {}

#[cfg(test)]
mod adapter_tests {
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

        assert_eq!(claude.submit_input(), InputSequence::bytes(b"\r"));
        assert_eq!(codex.submit_input(), InputSequence::bytes(b"\r"));
        assert_eq!(claude.queue_input(), InputSequence::bytes(b"\r"));
        assert_eq!(codex.queue_input(), InputSequence::bytes(b"\t"));
        assert_eq!(claude.new_session_input(), InputSequence::bytes(b"/new\r"));
        assert_eq!(codex.new_session_input(), InputSequence::bytes(b"/new\t"));
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

        assert_eq!(claude.completion_strategy(), CompletionStrategy::Hook);
        assert_eq!(codex.completion_strategy(), CompletionStrategy::Hook);
        assert_eq!(
            claude.transcript(&session),
            Some(PathBuf::from(
                "/home/tester/.claude/projects/-workspaces-family brain/sess-9.jsonl"
            ))
        );
        assert_eq!(codex.transcript(&session), None);
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

        assert_eq!(claude.response_id(&session), "sess-9");
        let codex_response_id = codex.response_id(&session);
        assert_ne!(codex_response_id, "sess-9");
        assert_eq!(codex.response_id(&session), codex_response_id);
        assert!(uuid::Uuid::parse_str(&codex_response_id).is_ok());
        assert!(claude.can_resume_response_session());
        assert!(!codex.can_resume_response_session());
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

        assert!(
            claude.resume_candidate_exists(&AgentSession::new("valid").expect("valid session"))
        );
        assert!(
            !claude
                .resume_candidate_exists(&AgentSession::new("missing").expect("missing session"))
        );
        assert!(
            !codex
                .resume_candidate_exists(&AgentSession::new("unvalidated").expect("Codex session"))
        );
    }
}
