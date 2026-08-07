use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use brain::{
    access::AccessMode,
    actor::{ActorContext, RequestIdentity},
    agent::{
        AgentController, AgentError, AgentKind, AgentSession, AgentTransport, CompletionStrategy,
        InputSequence, LaunchRequest, LaunchSpec, SessionPlan,
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
    for arguments in [
        vec!["--codex", "--open-code"],
        vec!["-cx", "-oc"],
        vec!["--codex", "tasks", "today", "-oc"],
        vec!["tasks", "--open-code", "today", "--codex"],
        vec!["tasks", "today", "-cx", "--open-code"],
        vec!["--codex", "-cx", "tasks", "today", "-oc"],
    ] {
        let home = tempfile::tempdir().expect("temporary home");
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_brain"))
            .args(&arguments)
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", home.path().join("config"))
            .env("NO_COLOR", "1")
            .output()
            .expect("run brain conflict");

        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        assert!(
            output.stdout.is_empty(),
            "{arguments:?}: {:?}",
            output.stdout
        );
        assert_eq!(
            String::from_utf8(output.stderr).expect("UTF-8 stderr"),
            "🔴 Choose one agent frontend: --codex or --open-code.\n",
            "{arguments:?}"
        );
        assert_eq!(
            std::fs::read_dir(home.path())
                .expect("read temporary home")
                .count(),
            0,
            "selection conflict must precede registry, hook, server, and TUI setup: {arguments:?}"
        );
    }
}

#[test]
fn opencode_input_and_workspace_scoped_session_contracts_are_explicit() {
    let temporary = tempfile::tempdir().expect("temporary workspace");
    let workspace = workspace(temporary.path());
    std::fs::create_dir_all(workspace.root()).expect("workspace root");
    let log = temporary.path().join("invocations.log");
    let command = fake_command(&log);
    let (mut controller, recording) = recording_controller(&workspace, &command);
    let session = AgentSession::new("session-1").expect("session");

    controller.submit_now().expect("submit");
    controller
        .queue_after_active_turn("next")
        .expect("busy-turn follow-up");
    controller.start_new_session().expect("new session");
    assert_eq!(
        recording.inputs.lock().expect("recorded input").as_slice(),
        [b"\r".to_vec(), b"next\r".to_vec(), b"/new\r".to_vec()]
    );
    assert_eq!(controller.kind(), AgentKind::OpenCode);
    assert_eq!(
        controller.completion_strategy(),
        Ok(CompletionStrategy::Hook)
    );
    assert!(
        controller
            .resume_candidate_exists(&session)
            .expect("session validation")
    );
    assert!(
        !controller
            .resume_candidate_exists(&AgentSession::new("stale").unwrap())
            .expect("stale session validation")
    );
    assert!(
        !controller
            .resume_candidate_exists(&AgentSession::new("child").unwrap())
            .expect("child session validation")
    );
    assert!(
        controller
            .can_resume_response_session(&session)
            .expect("resume support")
    );
    let invocations = std::fs::read_to_string(&log).expect("invocation log");
    let session_invocations = invocations
        .lines()
        .filter(|line| line.ends_with("|session list --format json"))
        .collect::<Vec<_>>();
    assert_eq!(
        session_invocations.len(),
        2,
        "candidate validation shares one snapshot; receiver validation is fresh"
    );
    let canonical_workspace = workspace
        .root()
        .canonicalize()
        .expect("canonical workspace");
    assert!(session_invocations.iter().all(|line| {
        let (cwd, arguments) = line.split_once('|').expect("recorded invocation");
        Path::new(cwd)
            .canonicalize()
            .is_ok_and(|path| path == canonical_workspace)
            && arguments == "session list --format json"
    }));
    let first = controller.response_id(&session).expect("response identity");
    assert_eq!(
        first,
        controller.response_id(&session).expect("stable identity")
    );
    assert!(uuid::Uuid::parse_str(&first).is_ok());
}

fn shell_word(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

#[test]
fn opencode_rejects_a_missing_command_with_an_actionable_prelaunch_error() {
    let temporary = tempfile::tempdir().expect("temporary workspace");
    let workspace = workspace(temporary.path());
    let (controller, _recording) = recording_controller(&workspace, "missing-opencode-binary");

    let error = controller
        .ensure_available()
        .expect_err("missing OpenCode command must fail preflight");

    assert_eq!(
        error.to_string(),
        "frontend error: OpenCode is unavailable: the configured command could not run. Install OpenCode or set `brain env set opencode_cmd <command>`."
    );
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
    )
    .with_capability_plan(
        brain::access::capability_plan(
            &brain::config::Config {
                access_mode: AccessMode::WorkspaceOnly,
                allowed_mcps: Vec::new(),
                allowed_skills: Vec::new(),
                ..brain::config::Config::default()
            },
            &brain::access::MachineCapabilityEnvironment::from_value(
                workspace.id(),
                serde_json::json!({}),
            )
            .expect("empty machine capabilities"),
        )
        .expect("empty capability plan"),
    );
    let log = temporary.path().join("invocations.log");
    let command = fake_command(&log);
    let (mut controller, recording) = recording_controller(&workspace, &command);
    controller.launch(&request).expect("OpenCode launch");
    let spec = recording
        .launches
        .lock()
        .expect("recorded launch")
        .first()
        .cloned()
        .expect("OpenCode launch spec");
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
    assert!(
        spec.command
            .ends_with("--agent brain --prompt 'untrusted --prompt text'")
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

fn fake_command(log: &Path) -> String {
    let fake =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/opencode/fake_opencode.sh");
    format!(
        "OPENCODE_TEST_LOG={} {}",
        shell_word(log),
        shell_word(&fake)
    )
}

fn recording_controller(
    workspace: &Arc<WorkspaceContext>,
    command: &str,
) -> (AgentController, Arc<Recording>) {
    let recording = Arc::new(Recording::default());
    let controller = AgentController::for_workspace_with_command(
        Arc::clone(workspace),
        AgentKind::OpenCode,
        command.to_owned(),
        actor(),
        Box::new(RecordingTransport {
            recording: Arc::clone(&recording),
        }),
    );
    (controller, recording)
}

#[derive(Default)]
struct Recording {
    launches: Mutex<Vec<LaunchSpec>>,
    inputs: Mutex<Vec<Vec<u8>>>,
}

struct RecordingTransport {
    recording: Arc<Recording>,
}

impl AgentTransport for RecordingTransport {
    fn spawn(&mut self, spec: &LaunchSpec) -> Result<(), AgentError> {
        self.recording
            .launches
            .lock()
            .expect("recorded launches")
            .push(spec.clone());
        Ok(())
    }

    fn send(&mut self, input: InputSequence) -> Result<(), AgentError> {
        self.recording
            .inputs
            .lock()
            .expect("recorded inputs")
            .push(input.into_bytes());
        Ok(())
    }

    fn snapshot(&self) -> String {
        String::new()
    }

    fn is_alive(&self) -> bool {
        true
    }

    fn shutdown(&mut self) {}
}
