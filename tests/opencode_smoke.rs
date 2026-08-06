use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use brain::{
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
fn open_code_selection_exits_unsupported_before_startup_side_effects() {
    let home = tempfile::tempdir().expect("temporary home");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_brain"))
        .arg("--open-code")
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env("NO_COLOR", "1")
        .output()
        .expect("run brain OpenCode selection");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty(), "stdout: {:?}", output.stdout);
    assert_eq!(
        String::from_utf8(output.stderr).expect("UTF-8 stderr"),
        "🔴 OpenCode is not supported\n"
    );
    assert_eq!(
        std::fs::read_dir(home.path())
            .expect("read temporary home")
            .count(),
        0,
        "OpenCode rejection must precede registry, hook, server, and TUI setup"
    );
}

#[test]
fn opencode_frontend_rejects_every_frontend_operation() {
    let temporary = tempfile::tempdir().expect("temporary workspace");
    let workspace = workspace(temporary.path());
    let actor = actor();
    let request = launch_request(
        Arc::clone(&workspace),
        actor,
        SessionPlan::fresh(AgentSession::new("fresh-1").expect("session")),
    );
    let session = AgentSession::new("session-1").expect("session");
    let frontend = OpenCodeFrontend::new("opencode --model future");

    assert_eq!(frontend.kind(), AgentKind::OpenCode);
    assert_eq!(frontend.kind().label(), "OpenCode");
    assert_unsupported(frontend.launch_spec(&request));
    assert_unsupported(frontend.submit_input());
    assert_unsupported(frontend.queue_input());
    assert_unsupported(frontend.new_session_input());
    assert_unsupported(frontend.completion_strategy());
    assert_unsupported(frontend.transcript(&session));
    assert_unsupported(frontend.resume_candidate_exists(&session));
    assert_unsupported(frontend.response_id(&session));
    assert_unsupported(frontend.can_resume_response_session());
}

#[test]
fn opencode_controller_is_constructible_but_every_lifecycle_and_input_fails_without_effects() {
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
        actor.clone(),
        SessionPlan::fresh(AgentSession::new("fresh-1").expect("session")),
    );
    let resumed = launch_request(
        workspace,
        actor,
        SessionPlan::resume(AgentSession::new("resume-1").expect("session")),
    );
    let session = AgentSession::new("session-1").expect("session");

    assert_eq!(controller.kind(), AgentKind::OpenCode);
    assert_unsupported(controller.launch(&fresh));
    assert_unsupported(controller.launch(&resumed));
    assert_unsupported(controller.type_text("hello"));
    assert_unsupported(controller.forward_terminal_input(vec![b'x']));
    assert_unsupported(controller.submit_now());
    assert_unsupported(controller.queue_after_active_turn("next"));
    assert_unsupported(controller.tick());
    assert_unsupported(controller.start_new_session());
    assert_unsupported(controller.completion_strategy());
    assert_unsupported(controller.transcript(&session));
    assert_unsupported(controller.snapshot());
    assert_unsupported(controller.is_alive());
    assert_unsupported(controller.resume_candidate_exists(&session));
    assert_unsupported(controller.response_id(&session));
    assert_unsupported(controller.can_resume_response_session());
    assert_unsupported(controller.terminal_screen());
    assert_unsupported(controller.resize(24, 80));
    assert_unsupported(controller.scroll_up(3));
    assert_unsupported(controller.scroll_down(3));
    assert_unsupported(controller.scroll_to_bottom());
    assert_unsupported(controller.terminal_rows());
    assert_unsupported(controller.shutdown());

    assert!(
        effects.lock().expect("effects lock").is_empty(),
        "the stub must not reach any transport side effect"
    );
}

fn assert_unsupported<T>(result: Result<T, AgentError>) {
    match result {
        Err(error) => assert_eq!(error, AgentError::UnsupportedFrontend(AgentKind::OpenCode)),
        Ok(_) => panic!("OpenCode operation must fail"),
    }
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
