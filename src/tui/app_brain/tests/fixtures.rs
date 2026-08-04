use super::*;

const WORKSPACE_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ControllerEvent {
    SubmitNow,
    QueueAfterActiveTurn,
    QueueDelivered,
    StartNewSession,
    Shutdown,
}

#[derive(Clone, Default)]
pub(super) struct ControllerRecording(Arc<Mutex<Vec<ControllerEvent>>>);

impl ControllerRecording {
    fn record(&self, event: ControllerEvent) {
        self.0.lock().expect("controller recording").push(event);
    }

    pub(super) fn events(&self) -> Vec<ControllerEvent> {
        self.0.lock().expect("controller recording").clone()
    }
}

struct RecordingFrontend {
    recording: ControllerRecording,
}

impl AgentFrontend for RecordingFrontend {
    fn kind(&self) -> AgentKind {
        AgentKind::Claude
    }

    fn launch_spec(&self, request: &LaunchRequest) -> Result<LaunchSpec, AgentError> {
        Ok(LaunchSpec::new(
            "recording-agent",
            request.workspace().root().to_path_buf(),
            Vec::new(),
            HookMetadata::none(),
        ))
    }

    fn submit_input(&self) -> InputSequence {
        self.recording.record(ControllerEvent::SubmitNow);
        InputSequence::bytes(b"\r")
    }

    fn queue_input(&self) -> InputSequence {
        self.recording.record(ControllerEvent::QueueAfterActiveTurn);
        InputSequence::bytes(b"\x1dqueue")
    }

    fn new_session_input(&self) -> InputSequence {
        self.recording.record(ControllerEvent::StartNewSession);
        InputSequence::bytes(b"/new\r")
    }

    fn completion_strategy(&self) -> CompletionStrategy {
        CompletionStrategy::Hook
    }

    fn transcript(&self, _session: &AgentSession) -> Option<PathBuf> {
        None
    }

    fn resume_candidate_exists(&self, _session: &AgentSession) -> bool {
        true
    }

    fn response_id(&self, session: &AgentSession) -> String {
        session.as_str().to_owned()
    }

    fn can_resume_response_session(&self) -> bool {
        true
    }
}

struct RecordingTransport {
    recording: ControllerRecording,
    alive: bool,
    snapshot: String,
}

impl AgentTransport for RecordingTransport {
    fn spawn(&mut self, _spec: &LaunchSpec) -> Result<(), AgentError> {
        self.alive = true;
        Ok(())
    }

    fn send(&mut self, input: InputSequence) -> Result<(), AgentError> {
        if input.into_bytes().ends_with(b"\x1dqueue") {
            self.recording.record(ControllerEvent::QueueDelivered);
        }
        Ok(())
    }

    fn snapshot(&self) -> String {
        self.snapshot.clone()
    }

    fn is_alive(&self) -> bool {
        self.alive
    }

    fn shutdown(&mut self) {
        self.recording.record(ControllerEvent::Shutdown);
        self.alive = false;
    }
}

#[derive(Clone, Default)]
pub(super) struct LaunchRecording(pub(super) Arc<Mutex<Vec<LaunchSpec>>>);

pub(super) struct LaunchRecordingTransport {
    pub(super) recording: LaunchRecording,
    pub(super) alive: bool,
}

impl AgentTransport for LaunchRecordingTransport {
    fn spawn(&mut self, spec: &LaunchSpec) -> Result<(), AgentError> {
        self.recording
            .0
            .lock()
            .expect("launch recording")
            .push(spec.clone());
        self.alive = true;
        Ok(())
    }

    fn send(&mut self, _input: InputSequence) -> Result<(), AgentError> {
        Ok(())
    }

    fn snapshot(&self) -> String {
        String::new()
    }

    fn is_alive(&self) -> bool {
        self.alive
    }

    fn shutdown(&mut self) {
        self.alive = false;
    }
}

pub(super) fn recording_controller(
    app: &App<'_>,
    alive: bool,
    snapshot: &str,
) -> (AgentController, ControllerRecording) {
    recording_controller_for_actor(app, app.interactive_actor.clone(), alive, snapshot)
}

pub(super) fn recording_controller_for_actor(
    app: &App<'_>,
    actor: crate::actor::ActorContext,
    alive: bool,
    snapshot: &str,
) -> (AgentController, ControllerRecording) {
    let recording = ControllerRecording::default();
    let controller = AgentController::new(
        Arc::clone(&app.command_context.workspace),
        actor,
        Box::new(RecordingFrontend {
            recording: recording.clone(),
        }),
        Box::new(RecordingTransport {
            recording: recording.clone(),
            alive,
            snapshot: snapshot.to_owned(),
        }),
    );
    (controller, recording)
}

pub(super) fn test_app<'a>(
    temporary: &tempfile::TempDir,
    cli: &'a Cli,
    agent_kind: AgentKind,
) -> App<'a> {
    let root = temporary.path().join("family");
    std::fs::create_dir_all(root.join("tasks")).expect("create task directory");
    std::fs::create_dir_all(root.join(".config")).expect("create config directory");
    std::fs::write(
        root.join("tasks/tasks.csv"),
        "task_uuid,task_id,task_name,status,assigned_to,system_key\n",
    )
    .expect("write tasks");
    std::fs::write(
        root.join("tasks/habits.csv"),
        "task_uuid,task_id,task_name,status,assigned_to,system_key\n",
    )
    .expect("write habits");
    std::fs::write(
        root.join(".config/config.json"),
        "{\"claude_cmd\":\"sh -c 'sleep 30' #\",\"codex_cmd\":\"codex-test\"}\n",
    )
    .expect("write test agent command");
    let workspace = WorkspaceContext::new(
        temporary.path(),
        WorkspaceId::parse(WORKSPACE_ID).expect("valid workspace id"),
        WorkspaceName::parse("family").expect("valid workspace name"),
        &root,
        "pablo",
        temporary.path(),
    )
    .expect("workspace context");
    let context = CommandContext::for_test(
        Arc::new(workspace),
        RegistryStore::from_path(temporary.path().join("env.json")),
        "pablo",
    );
    let today = NaiveDate::from_ymd_opt(2026, 8, 4).expect("valid date");
    let view = build_view(cli, &Selector::All, Some(View::All), Vec::new(), today);
    let assignment = AssignmentContext::legacy(&context.actor);
    let db = Db::open(&context.workspace).expect("state db");
    App::new(
        context,
        &view,
        cli,
        today,
        root.join("tasks/tasks.csv"),
        Vec::new(),
        Vec::new(),
        assignment,
        None,
        Some(View::All),
        None,
        Box::new(ZshFunctionRunner::new("")),
        Box::new(ZshFunctionRunner::new("")),
        Config {
            enable_triage_habits: false,
            ..Config::default()
        },
        agent_kind,
        "shell-under-test".to_owned(),
        db,
        crate::picker::App::new(&[], ""),
        PanelSide::Right,
        true,
    )
}

pub(super) fn sms_actor() -> crate::actor::ActorContext {
    let users = crate::users::Users {
        schema_version: crate::users::USERS_SCHEMA_VERSION,
        users: vec![crate::users::User {
            id: crate::users::UserId::parse("remote-member").unwrap(),
            name: "Remote member".to_owned(),
            phones: vec![crate::users::PhoneIdentity {
                value: "+15551234567".to_owned(),
                inbound_allowed: true,
            }],
            emails: Vec::new(),
            response_email: None,
        }],
    };
    crate::actor::resolve_actor(
        &crate::users::UserId::parse("remote-member").unwrap(),
        crate::actor::RequestIdentity::Sms {
            from: "+15551234567",
        },
        &users,
    )
    .unwrap()
}

pub(super) fn live_panel(root: &Path) -> PtyPane {
    PtyPane::spawn_shell_command_with_env("cat", &[], root, 24, 80).expect("spawn panel")
}

pub(super) fn panel_controller(app: &App<'_>, panel: PtyPane) -> AgentController {
    AgentController::new(
        Arc::clone(&app.command_context.workspace),
        app.interactive_actor.clone(),
        crate::agent::configured_frontend(&app.command_context, app.agent_kind),
        Box::new(panel),
    )
}

pub(super) struct FailingSessionStore;

impl SessionStore for FailingSessionStore {
    fn reap_dead_locks(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn sessions_by_recency(&self, _scope: &SessionScope) -> Vec<String> {
        Vec::new()
    }

    fn claim(
        &self,
        _session: &AgentSession,
        _instance: &str,
        _pid: i32,
        _scope: &SessionScope,
    ) -> anyhow::Result<bool> {
        Ok(false)
    }

    fn register(
        &self,
        _session: &AgentSession,
        _instance: &str,
        _pid: i32,
        _scope: &SessionScope,
    ) -> anyhow::Result<()> {
        anyhow::bail!("authorization store unavailable")
    }

    fn release(&self, _instance: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn mark_active(&self, _instance: &str, _scope: &SessionScope) -> anyhow::Result<bool> {
        Ok(false)
    }

    fn mark_completed(
        &self,
        _session: &AgentSession,
        _scope: &SessionScope,
    ) -> anyhow::Result<bool> {
        Ok(false)
    }

    fn completion_status(
        &self,
        _session: &AgentSession,
        _scope: &SessionScope,
    ) -> Option<crate::agent::CompletionStatus> {
        None
    }
}

pub(super) fn capture_panel(root: &Path) -> PtyPane {
    PtyPane::spawn_shell_command_with_env(
        "stty raw -echo; printf READY; dd bs=1 count=5 2>/dev/null | od -An -t x1",
        &[],
        root,
        24,
        80,
    )
    .expect("spawn capture panel")
}

pub(super) fn wait_for_panel_contents(panel: &AgentController, expected: &str) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let normalized = panel
            .snapshot()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if normalized.contains(expected) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

pub(super) struct ClaudeTranscript {
    path: PathBuf,
    project_dir: PathBuf,
}

impl ClaudeTranscript {
    pub(super) fn create(brain_root: &Path, session_id: &str) -> Self {
        let home = std::env::var_os("HOME").expect("test home directory");
        let project_dir = PathBuf::from(home)
            .join(".claude/projects")
            .join(session::project_dir_name(brain_root));
        std::fs::create_dir_all(&project_dir).expect("create transcript directory");
        let path = project_dir.join(format!("{session_id}.jsonl"));
        std::fs::write(&path, "{}\n").expect("write Claude transcript");
        Self { path, project_dir }
    }
}

impl Drop for ClaudeTranscript {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_dir(&self.project_dir);
    }
}
