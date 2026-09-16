//! The facade driving a real fake `pi` process: the argv pi actually receives,
//! the bytes its terminal actually gets, and the environment it runs in.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Arc,
};

use brain::{
    access::AccessMode,
    agent::{
        AgentController, AgentError, AgentKind, AgentSession, AgentTransport, InputSequence,
        LaunchRequest, LaunchSpec, SessionPlan,
    },
    users::{USERS_SCHEMA_VERSION, User, UserId, Users, UsersStore},
    workspace::{
        CommandContext, MachineRegistry, REGISTRY_SCHEMA_VERSION, RegistryStore, WorkspaceContext,
        WorkspaceId, WorkspaceName, WorkspaceRecord,
    },
};

struct ProcessTransport {
    child: Option<Child>,
}

impl ProcessTransport {
    const fn new() -> Self {
        Self { child: None }
    }
}

impl AgentTransport for ProcessTransport {
    fn spawn(&mut self, spec: &LaunchSpec) -> Result<(), AgentError> {
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", &spec.command])
            .current_dir(&spec.cwd)
            .envs(spec.environment.iter().cloned())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        self.child = Some(
            command
                .spawn()
                .map_err(|error| AgentError::Transport(error.to_string()))?,
        );
        Ok(())
    }

    fn send(&mut self, input: InputSequence) -> Result<(), AgentError> {
        let stdin = self
            .child
            .as_mut()
            .and_then(|child| child.stdin.as_mut())
            .ok_or_else(|| AgentError::Transport("pi stdin is unavailable".to_owned()))?;
        stdin
            .write_all(&input.flattened())
            .and_then(|()| stdin.flush())
            .map_err(|error| AgentError::Transport(error.to_string()))
    }

    fn snapshot(&self) -> String {
        String::new()
    }

    fn is_alive(&self) -> bool {
        self.child.is_some()
    }

    fn shutdown(&mut self) -> Result<(), AgentError> {
        if let Some(mut child) = self.child.take() {
            drop(child.stdin.take());
            child.wait().expect("fake pi exits after stdin closes");
        }
        Ok(())
    }
}

#[test]
fn facade_drives_a_real_fake_pi_process_with_semantic_input() {
    let fixture = Fixture::new();
    let mut controller = AgentController::configured(
        &fixture.command,
        AgentKind::Pi,
        fixture.command.actor.clone(),
        Box::new(ProcessTransport::new()),
    );
    let request = fixture.request(
        SessionPlan::fresh(AgentSession::new("fresh-pi-session").unwrap()),
        Some("initial text stays one argument"),
    );

    controller.launch(&request).expect("fresh pi launch");
    controller
        .type_text("literal input")
        .expect("literal typing");
    controller.submit_now().expect("immediate submit");
    controller
        .queue_after_active_turn("busy follow-up")
        .expect("native busy-turn follow-up");
    controller.start_new_session().expect("new session command");
    controller.shutdown().expect("first shutdown");
    controller.shutdown().expect("idempotent shutdown");

    let log = fixture.log();
    assert!(log.contains("launch|"), "{log}");
    // Project-local trust is declined, Brain's own extension is loaded by path,
    // and Brain's chosen session id identifies the conversation.
    assert!(log.contains("arg|0|--no-approve"), "{log}");
    assert!(log.contains("arg|1|--extension"), "{log}");
    assert!(
        log.contains(".brain/hooks/pi_brain_extension.ts"),
        "{log}"
    );
    assert!(log.contains("arg|3|--session-id"), "{log}");
    assert!(log.contains("arg|4|fresh-pi-session"), "{log}");
    // `--` keeps an option-looking prompt one argument.
    assert!(log.contains("arg|5|--"), "{log}");
    assert!(
        log.contains("arg|6|initial text stays one argument"),
        "{log}"
    );
    // ESC[200~literal input ESC[201~ CR
    //   ESC[200~busy follow-up ESC[201~ ESC[13;3u
    //   /new CR
    // The submit key is Enter; the busy-turn follow-up is pi's Alt+Enter queue
    // in its CSI u form, which is why those two differ.
    assert!(
        log.contains(
            "input|1b5b3230307e6c69746572616c20696e7075741b5b3230317e0d\
             1b5b3230307e6275737920666f6c6c6f772d75701b5b3230317e1b5b31333b3375\
             2f6e65770d"
        ),
        "{log}"
    );
    assert!(log.contains("env|BRAIN_ACTOR_ID"), "{log}");
    assert!(log.contains("env|BRAIN_AGENT_KIND"), "{log}");
    assert!(log.contains("env|BRAIN_ROOT"), "{log}");
    assert!(!log.contains("initial-provider-secret"), "{log}");
}

/// pi's `--session-id` opens an existing session and creates a missing one, so
/// a resumed launch names the same flag with the id Brain validated.
#[test]
fn facade_resumes_a_recorded_session_through_the_same_session_flag() {
    let fixture = Fixture::new();
    let mut controller = AgentController::configured(
        &fixture.command,
        AgentKind::Pi,
        fixture.command.actor.clone(),
        Box::new(ProcessTransport::new()),
    );
    // Resume *evidence* is the session file pi writes, unit-tested in
    // `agent::pi::sessions`; this is about the command a validated plan builds.
    let session = AgentSession::new("recorded-pi-session").unwrap();
    let request = fixture.request(SessionPlan::resume(session), Some("resume safely"));

    controller.launch(&request).expect("resumed pi launch");
    controller.shutdown().expect("shutdown");

    let log = fixture.log();
    assert!(log.contains("arg|3|--session-id"), "{log}");
    assert!(log.contains("arg|4|recorded-pi-session"), "{log}");
    assert!(log.contains("arg|5|--"), "{log}");
    assert!(log.contains("arg|6|resume safely"), "{log}");
}

struct Fixture {
    _temporary: tempfile::TempDir,
    command: CommandContext,
    log_path: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().expect("temporary acceptance root");
        let home = temporary.path().join("home");
        let root = temporary.path().join("family");
        std::fs::create_dir_all(&root).unwrap();
        let id = WorkspaceId::parse("11111111-1111-4111-8111-111111111111").unwrap();
        let name = WorkspaceName::parse("family").unwrap();
        let workspace = Arc::new(
            WorkspaceContext::new(&home, id, name.clone(), &root, "pablo", &root).unwrap(),
        );
        UsersStore::save(
            &workspace,
            &Users {
                schema_version: USERS_SCHEMA_VERSION,
                users: vec![User {
                    id: UserId::parse("pablo").unwrap(),
                    name: "Pablo".to_owned(),
                    phones: Vec::new(),
                    emails: Vec::new(),
                    response_email: None,
                }],
            },
        )
        .unwrap();
        let log_path = temporary.path().join("pi.log");
        let fake = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pi/fake_pi.sh");
        let configured = format!(
            "PI_TEST_LOG={} INITIAL_PROVIDER_TOKEN=initial-provider-secret {}",
            shell_word(&log_path),
            shell_word(&fake)
        );
        let store = RegistryStore::from_path(home.join(".config/brain/env.json"));
        store
            .replace(&MachineRegistry {
                schema_version: REGISTRY_SCHEMA_VERSION,
                default_workspace: name.clone(),
                workspaces: BTreeMap::from([(
                    name,
                    WorkspaceRecord {
                        workspace_id: id,
                        root,
                        aliases: BTreeSet::new(),
                        local_user_id: "pablo".to_owned(),
                        receiver_enabled: false,
                        env: serde_json::Map::from_iter([(
                            "pi_cmd".to_owned(),
                            serde_json::Value::String(configured),
                        )]),
                    },
                )]),
                env: serde_json::Map::new(),
            })
            .unwrap();
        let command = CommandContext::new(workspace, store).unwrap();
        Self {
            _temporary: temporary,
            command,
            log_path,
        }
    }

    fn request(&self, plan: SessionPlan, prompt: Option<&str>) -> LaunchRequest {
        LaunchRequest::from_trusted_context(
            Arc::clone(&self.command.workspace),
            self.command.actor.clone(),
            plan,
            prompt.map(str::to_owned),
            AccessMode::Unrestricted,
        )
    }

    fn log(&self) -> String {
        std::fs::read_to_string(&self.log_path).expect("fake pi log")
    }
}

fn shell_word(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}
