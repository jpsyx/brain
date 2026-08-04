use std::path::{Path, PathBuf};
use std::sync::Arc;

use brain::access::AccessMode;
use brain::actor::{RequestIdentity, resolve_actor};
use brain::agent::{
    AgentFrontend, AgentSession, ClaudeFrontend, CodexFrontend, LaunchRequest, SessionPlan,
};
use brain::users::{USERS_SCHEMA_VERSION, User, UserId, Users};
use brain::workspace::{WorkspaceContext, WorkspaceId, WorkspaceName};

fn workspace() -> Arc<WorkspaceContext> {
    Arc::new(
        WorkspaceContext::new(
            Path::new("/Users/test"),
            WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").expect("valid id"),
            WorkspaceName::parse("family").expect("valid name"),
            Path::new("/Users/test/family"),
            "pablo",
            Path::new("/Users/test"),
        )
        .expect("workspace context"),
    )
}

fn actor() -> brain::actor::ActorContext {
    let pablo = UserId::parse("pablo").expect("valid user id");
    let users = Users {
        schema_version: USERS_SCHEMA_VERSION,
        users: vec![User {
            id: pablo.clone(),
            name: "Pablo".to_owned(),
            phones: Vec::new(),
            emails: Vec::new(),
            response_email: None,
        }],
    };
    resolve_actor(&pablo, RequestIdentity::Local, &users).expect("actor")
}

fn request(plan: SessionPlan, mode: AccessMode) -> LaunchRequest {
    LaunchRequest::from_trusted_context(
        workspace(),
        actor(),
        plan,
        Some("User prompt stays separate".to_owned()),
        mode,
    )
}

#[test]
fn adapters_install_workspace_boundary_as_trusted_frontend_instructions() {
    let cases: Vec<(Box<dyn AgentFrontend>, LaunchRequest, &str)> = vec![
        (
            Box::new(ClaudeFrontend::new(
                "claude",
                PathBuf::from("/Users/test/family"),
                PathBuf::from("/Users/test/.claude/projects"),
            )),
            request(
                SessionPlan::fresh(AgentSession::new("claude-fresh").expect("session")),
                AccessMode::WorkspaceOnly,
            ),
            "--append-system-prompt",
        ),
        (
            Box::new(CodexFrontend::new("codex")),
            request(
                SessionPlan::resume(AgentSession::new("codex-resume").expect("session")),
                AccessMode::WorkspaceOnly,
            ),
            "developer_instructions",
        ),
    ];

    for (frontend, request, trusted_instruction_flag) in cases {
        let spec = frontend.launch_spec(&request).expect("launch spec");
        let boundary_position = spec
            .command
            .find("This is advisory prompt enforcement, not a filesystem sandbox.")
            .expect("boundary reaches frontend command");
        let user_position = spec
            .command
            .find("User prompt stays separate")
            .expect("user prompt reaches frontend command");

        assert!(spec.command.contains(trusted_instruction_flag));
        assert!(boundary_position < user_position);
        assert_eq!(spec.cwd, Path::new("/Users/test/family"));
    }
}

#[test]
fn unrestricted_launches_do_not_add_boundary_instruction_flags() {
    let claude = ClaudeFrontend::new(
        "claude",
        PathBuf::from("/Users/test/family"),
        PathBuf::from("/Users/test/.claude/projects"),
    );
    let codex = CodexFrontend::new("codex");
    let request = request(
        SessionPlan::fresh(AgentSession::new("unrestricted").expect("session")),
        AccessMode::Unrestricted,
    );

    assert!(
        !claude
            .launch_spec(&request)
            .expect("Claude launch")
            .command
            .contains("--append-system-prompt")
    );
    assert!(
        !codex
            .launch_spec(&request)
            .expect("Codex launch")
            .command
            .contains("developer_instructions")
    );
}

#[test]
fn launch_environment_contains_only_selected_context_and_frontend_necessities() {
    let frontend = CodexFrontend::new("codex");
    let request = request(
        SessionPlan::fresh(AgentSession::new("minimal-env").expect("session")),
        AccessMode::WorkspaceOnly,
    );

    let spec = frontend.launch_spec(&request).expect("launch spec");
    let keys = spec
        .environment
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>();
    let permitted_runtime = [
        "HOME",
        "PATH",
        "SHELL",
        "USER",
        "LOGNAME",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
        "TMPDIR",
        "SSH_AUTH_SOCK",
    ];
    let selected_brain_context = [
        "BRAIN_WORKSPACE_ID",
        "BRAIN_WORKSPACE",
        "BRAIN_ROOT",
        "BRAIN_ACTOR_ID",
        "BRAIN_CHANNEL",
        "BRAIN_AGENT_KIND",
    ];

    assert!(keys.contains(&"HOME"));
    assert!(keys.contains(&"PATH"));
    assert!(keys.contains(&"BRAIN_ROOT"));
    assert!(keys.contains(&"BRAIN_AGENT_KIND"));
    assert!(
        keys.iter()
            .all(|key| permitted_runtime.contains(key) || selected_brain_context.contains(key))
    );
    assert!(!keys.contains(&"OPENAI_API_KEY"));
    assert!(!keys.contains(&"ANTHROPIC_API_KEY"));
    assert!(!keys.contains(&"BRAIN_WORKSPACE_REGISTRY"));
    assert!(
        spec.environment
            .iter()
            .all(|(_, value)| !value.contains("default_workspace"))
    );
}
