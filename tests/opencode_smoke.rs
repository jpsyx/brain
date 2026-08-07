use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use brain::{
    access::AccessMode,
    actor::{ActorContext, RequestIdentity},
    agent::{
        AccessPolicy, AgentController, AgentError, AgentFrontend, AgentKind, AgentSession,
        AgentTransport, LaunchRequest, LaunchSpec, OpenCodeFrontend, SessionPlan,
    },
    cli::{AgentSelectionError, try_parse_from},
    theme::Theme,
    users::{USERS_SCHEMA_VERSION, User, UserId, Users},
    workspace::{WorkspaceContext, WorkspaceId, WorkspaceName},
};

#[test]
fn open_code_flag_selects_only_opencode() {
    let cli = try_parse_from(["brain", "--open-code"]).expect("parse --open-code");

    assert_eq!(cli.selected_agent(), Ok(AgentKind::OpenCode));
}

#[test]
fn oc_alias_is_normalized_through_the_real_parser() {
    let cli = try_parse_from(["brain", "-oc"]).expect("parse -oc");

    assert_eq!(cli.selected_agent(), Ok(AgentKind::OpenCode));
}

#[test]
fn conflicting_frontend_flags_return_a_typed_exactly_rendered_error() {
    let cli = try_parse_from(["brain", "--codex", "--open-code"])
        .expect("both frontend flags parse before validation");

    let error = cli.selected_agent().expect_err("frontend conflict");
    assert_eq!(error, AgentSelectionError::ConflictingFrontends);
    assert_eq!(
        Theme::dark(false).error_line("🔴", &error.to_string()),
        "🔴 Choose one agent frontend: --codex or --open-code."
    );
}

#[test]
fn conflicting_frontend_flags_exit_before_startup_side_effects() {
    let home = tempfile::tempdir().expect("temporary home");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(["--codex", "--open-code"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env("NO_COLOR", "1")
        .output()
        .expect("run brain conflict");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty(), "stdout: {:?}", output.stdout);
    assert_eq!(
        String::from_utf8(output.stderr).expect("UTF-8 stderr"),
        "🔴 Choose one agent frontend: --codex or --open-code.\n"
    );
    assert_eq!(
        std::fs::read_dir(home.path())
            .expect("read temporary home")
            .count(),
        0,
        "selection conflict must precede registry, hook, server, and TUI setup"
    );
}

#[test]
fn opencode_builds_fresh_and_resumed_commands() {
    let temporary = tempfile::tempdir().expect("temporary workspace");
    let workspace = workspace(temporary.path());
    let actor = actor();
    let fresh = launch_request(
        Arc::clone(&workspace),
        actor.clone(),
        SessionPlan::fresh(AgentSession::new("fresh-1").expect("session")),
    );
    let resumed = LaunchRequest::new(
        workspace,
        actor,
        SessionPlan::resume(AgentSession::new("session-1").expect("session")),
        Some("don't lose this".to_owned()),
        AccessPolicy::default(),
    );
    let frontend = OpenCodeFrontend::new("opencode --model future");

    assert_eq!(frontend.kind(), AgentKind::OpenCode);
    assert_eq!(frontend.kind().label(), "OpenCode");
    assert_eq!(
        frontend.launch_spec(&fresh).expect("fresh launch").command,
        "opencode --model future --agent brain"
    );
    assert_eq!(
        frontend
            .launch_spec(&resumed)
            .expect("resume launch")
            .command,
        "opencode --model future --agent brain --session 'session-1' --prompt 'don'\\''t lose this'"
    );
}

#[test]
fn opencode_translates_semantic_input_and_session_identity() {
    let frontend = OpenCodeFrontend::new("opencode");
    let session = AgentSession::new("session-1").expect("session");

    assert_eq!(
        frontend.submit_input(),
        Ok(brain::agent::InputSequence::bytes(b"\r"))
    );
    assert_eq!(
        frontend.queue_input(),
        Ok(brain::agent::InputSequence::bytes(b"\r"))
    );
    assert_eq!(
        frontend.new_session_input(),
        Ok(brain::agent::InputSequence::bytes(b"/new\r"))
    );
    assert_eq!(
        frontend.completion_strategy(),
        Ok(brain::agent::CompletionStrategy::Hook)
    );
    assert_eq!(frontend.transcript(&session), Ok(None));
    assert!(
        frontend
            .resume_candidate_exists(&session)
            .expect("session validation")
    );
    assert!(
        frontend
            .can_resume_response_session()
            .expect("resume support")
    );
    let first = frontend.response_id(&session).expect("response identity");
    assert_eq!(
        first,
        frontend.response_id(&session).expect("stable identity")
    );
    assert!(uuid::Uuid::parse_str(&first).is_ok());
}

#[test]
fn opencode_puts_the_trusted_policy_in_its_named_agent_config() {
    let temporary = tempfile::tempdir().expect("temporary workspace");
    let workspace = workspace(temporary.path());
    let actor = actor();
    let request = LaunchRequest::from_trusted_context(
        Arc::clone(&workspace),
        actor,
        SessionPlan::fresh(AgentSession::new("fresh-1").expect("session")),
        Some("untrusted --prompt text".to_owned()),
        AccessMode::WorkspaceOnly,
    );
    let spec = OpenCodeFrontend::new("opencode")
        .launch_spec(&request)
        .expect("OpenCode launch");
    let config = spec
        .environment
        .iter()
        .find(|(name, _)| name == "OPENCODE_CONFIG_CONTENT")
        .map(|(_, value)| serde_json::from_str::<serde_json::Value>(value).expect("config JSON"))
        .expect("inline OpenCode config");

    assert!(
        config["agent"]["brain"]["prompt"]
            .as_str()
            .expect("Brain agent prompt")
            .contains("advisory prompt enforcement")
    );
    assert_eq!(
        spec.command,
        "opencode --agent brain --prompt 'untrusted --prompt text'"
    );
}

#[test]
fn opencode_controller_delegates_lifecycle_and_input_to_transport() {
    let temporary = tempfile::tempdir().expect("temporary workspace");
    let workspace = workspace(temporary.path());
    let actor = actor();
    let effects = Arc::new(Mutex::new(Vec::new()));
    let mut controller = AgentController::new(
        Arc::clone(&workspace),
        actor.clone(),
        Box::new(OpenCodeFrontend::new("opencode")),
        Box::new(RecordingTransport {
            effects: Arc::clone(&effects),
        }),
    );
    let fresh = launch_request(
        Arc::clone(&workspace),
        actor,
        SessionPlan::fresh(AgentSession::new("fresh-1").expect("session")),
    );
    let session = AgentSession::new("session-1").expect("session");

    assert_eq!(controller.kind(), AgentKind::OpenCode);
    controller.launch(&fresh).expect("fresh launch");
    controller.type_text("hello").expect("type");
    controller.submit_now().expect("submit");
    controller.queue_after_active_turn("next").expect("queue");
    controller.tick().expect("first queue tick");
    controller.tick().expect("second queue tick");
    controller.start_new_session().expect("new session");
    assert_eq!(
        controller.completion_strategy(),
        Ok(brain::agent::CompletionStrategy::Hook)
    );
    assert_eq!(controller.transcript(&session), Ok(None));
    assert_eq!(controller.snapshot(), Ok("snapshot".to_owned()));
    assert_eq!(controller.is_alive(), Ok(true));
    assert!(
        controller
            .resume_candidate_exists(&session)
            .expect("session validation")
    );
    assert!(
        controller
            .can_resume_response_session()
            .expect("resume support")
    );
    controller.terminal_screen().expect("screen");
    controller.resize(24, 80).expect("resize");
    controller.scroll_up(3).expect("scroll up");
    controller.scroll_down(3).expect("scroll down");
    controller.scroll_to_bottom().expect("scroll bottom");
    assert_eq!(controller.terminal_rows(), Ok(24));
    controller.shutdown().expect("shutdown");

    assert!(
        effects.lock().expect("effects lock").contains(&"spawn"),
        "functional OpenCode must reach the selected transport"
    );
}

fn workspace(root: &Path) -> Arc<WorkspaceContext> {
    let workspace_root = root.join("family");
    Arc::new(
        WorkspaceContext::new(
            root,
            WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").expect("workspace id"),
            WorkspaceName::parse("family").expect("workspace name"),
            &workspace_root,
            "pablo",
            root,
        )
        .expect("workspace context"),
    )
}

fn actor() -> ActorContext {
    let users = Users {
        schema_version: USERS_SCHEMA_VERSION,
        users: vec![User {
            id: UserId::parse("pablo").expect("user id"),
            name: "Pablo".to_owned(),
            phones: Vec::new(),
            emails: Vec::new(),
            response_email: None,
        }],
    };
    brain::actor::resolve_actor(
        &UserId::parse("pablo").expect("user id"),
        RequestIdentity::Local,
        &users,
    )
    .expect("actor")
}

fn launch_request(
    workspace: Arc<WorkspaceContext>,
    actor: ActorContext,
    plan: SessionPlan,
) -> LaunchRequest {
    LaunchRequest::new(workspace, actor, plan, None, AccessPolicy::default())
}

struct RecordingTransport {
    effects: Arc<Mutex<Vec<&'static str>>>,
}

impl RecordingTransport {
    fn record(&self, effect: &'static str) {
        self.effects.lock().expect("effects lock").push(effect);
    }
}

impl AgentTransport for RecordingTransport {
    fn spawn(&mut self, _spec: &LaunchSpec) -> Result<(), AgentError> {
        self.record("spawn");
        Ok(())
    }

    fn send(&mut self, _input: brain::agent::InputSequence) -> Result<(), AgentError> {
        self.record("send");
        Ok(())
    }

    fn snapshot(&self) -> String {
        self.record("snapshot");
        "snapshot".to_owned()
    }

    fn is_alive(&self) -> bool {
        self.record("is_alive");
        true
    }

    fn shutdown(&mut self) {
        self.record("shutdown");
    }

    fn terminal_screen(&self) -> Option<Arc<std::sync::RwLock<vt100::Parser>>> {
        self.record("terminal_screen");
        None
    }

    fn resize(&mut self, _rows: u16, _cols: u16) {
        self.record("resize");
    }

    fn scroll_up(&mut self, _rows: usize) {
        self.record("scroll_up");
    }

    fn scroll_down(&mut self, _rows: usize) {
        self.record("scroll_down");
    }

    fn scroll_to_bottom(&mut self) {
        self.record("scroll_to_bottom");
    }

    fn terminal_rows(&self) -> u16 {
        self.record("terminal_rows");
        24
    }
}
