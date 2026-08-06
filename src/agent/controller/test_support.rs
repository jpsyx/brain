use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use crate::{
    access::{AccessMode, MachineCapabilityEnvironment, capability_plan},
    actor::ActorContext,
    agent::{
        AccessPolicy, AgentController, AgentError, AgentFrontend, AgentKind, AgentSession,
        AgentTransport, CompletionStrategy, HookMetadata, InputSequence, LaunchRequest, LaunchSpec,
        SessionPlan,
    },
    config::Config,
    workspace::{WorkspaceContext, WorkspaceId, WorkspaceName},
};

const QUEUE_MARKER: &[u8] = b"\x1dqueue";
const NEW_SESSION_MARKER: &[u8] = b"\x1dnew";

#[derive(Debug, Clone, PartialEq, Eq)]
enum Event {
    Type(String),
    Submit,
    Queue(String),
    Launch(SessionPlan),
    Spawn,
    FrontendNewSession,
    TransportNewSession(InputSequence),
    Transcript(AgentSession),
    Shutdown,
}

#[derive(Clone, Default)]
struct Recording {
    events: Arc<Mutex<Vec<Event>>>,
}

impl Recording {
    fn record(&self, event: Event) {
        self.events.lock().expect("recording lock").push(event);
    }

    fn events(&self) -> Vec<Event> {
        self.events.lock().expect("recording lock").clone()
    }
}

struct RecordingFrontend {
    recording: Recording,
}

impl AgentFrontend for RecordingFrontend {
    fn kind(&self) -> AgentKind {
        AgentKind::Claude
    }

    fn launch_spec(&self, request: &LaunchRequest) -> Result<LaunchSpec, AgentError> {
        self.recording
            .record(Event::Launch(request.session_plan().clone()));
        Ok(LaunchSpec::new(
            "recording-agent",
            request.workspace().root().to_path_buf(),
            Vec::new(),
            HookMetadata::none(),
        ))
    }

    fn submit_input(&self) -> Result<InputSequence, AgentError> {
        self.recording.record(Event::Submit);
        Ok(InputSequence::bytes(b"\x1dsubmit"))
    }

    fn queue_input(&self) -> Result<InputSequence, AgentError> {
        Ok(InputSequence::bytes(QUEUE_MARKER))
    }

    fn new_session_input(&self) -> Result<InputSequence, AgentError> {
        self.recording.record(Event::FrontendNewSession);
        Ok(InputSequence::bytes(NEW_SESSION_MARKER))
    }

    fn completion_strategy(&self) -> Result<CompletionStrategy, AgentError> {
        Ok(CompletionStrategy::Hook)
    }

    fn transcript(&self, session: &AgentSession) -> Result<Option<PathBuf>, AgentError> {
        self.recording.record(Event::Transcript(session.clone()));
        Ok(Some(PathBuf::from("/transcripts").join(session.as_str())))
    }

    fn resume_candidate_exists(&self, _session: &AgentSession) -> Result<bool, AgentError> {
        Ok(true)
    }

    fn response_id(&self, session: &AgentSession) -> Result<String, AgentError> {
        Ok(session.as_str().to_owned())
    }

    fn can_resume_response_session(&self) -> Result<bool, AgentError> {
        Ok(true)
    }
}

struct RecordingTransport {
    recording: Recording,
    pending_text: Option<String>,
}

impl AgentTransport for RecordingTransport {
    fn spawn(&mut self, _spec: &LaunchSpec) -> Result<(), AgentError> {
        self.recording.record(Event::Spawn);
        Ok(())
    }

    fn send(&mut self, input: InputSequence) -> Result<(), AgentError> {
        if input == InputSequence::bytes(NEW_SESSION_MARKER) {
            self.recording.record(Event::TransportNewSession(input));
            return Ok(());
        }

        let bytes = input.into_bytes();
        if let Some(text) = bytes.strip_suffix(QUEUE_MARKER) {
            let text = if text.is_empty() {
                self.pending_text.take().unwrap_or_default()
            } else {
                String::from_utf8_lossy(text).into_owned()
            };
            self.recording.record(Event::Queue(text));
        } else if bytes != b"\x1dsubmit" {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            self.pending_text = Some(text.clone());
            self.recording.record(Event::Type(text));
        }
        Ok(())
    }

    fn snapshot(&self) -> String {
        "snapshot".to_owned()
    }

    fn is_alive(&self) -> bool {
        true
    }

    fn shutdown(&mut self) {
        self.recording.record(Event::Shutdown);
    }
}

fn workspace() -> Arc<WorkspaceContext> {
    Arc::new(
        WorkspaceContext::new(
            Path::new("/home/tester"),
            WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").expect("id"),
            WorkspaceName::parse("family").expect("name"),
            Path::new("/home/tester/family"),
            "pablo",
            Path::new("/home/tester"),
        )
        .expect("workspace"),
    )
}

fn controller() -> (
    AgentController,
    Recording,
    Arc<WorkspaceContext>,
    ActorContext,
) {
    let workspace = workspace();
    let actor = crate::actor::test_actor("pablo");
    let recording = Recording::default();
    let controller = AgentController::new(
        Arc::clone(&workspace),
        actor.clone(),
        Box::new(RecordingFrontend {
            recording: recording.clone(),
        }),
        Box::new(RecordingTransport {
            recording: recording.clone(),
            pending_text: None,
        }),
    );
    (controller, recording, workspace, actor)
}

fn request(
    workspace: Arc<WorkspaceContext>,
    actor: ActorContext,
    plan: SessionPlan,
) -> LaunchRequest {
    LaunchRequest::new(workspace, actor, plan, None, AccessPolicy::default())
}

fn trusted_request(
    workspace: Arc<WorkspaceContext>,
    actor: ActorContext,
    access_mode: AccessMode,
) -> LaunchRequest {
    LaunchRequest::from_trusted_context(
        workspace,
        actor,
        SessionPlan::fresh(AgentSession::new("capability-session").expect("session")),
        None,
        access_mode,
    )
}

fn capabilities(
    source_workspace: WorkspaceId,
    access_mode: AccessMode,
) -> crate::access::CapabilityPlan {
    let config = Config {
        access_mode,
        allowed_skills: Vec::new(),
        ..Config::default()
    };
    let machine = MachineCapabilityEnvironment::from_value(source_workspace, serde_json::json!({}))
        .expect("machine capabilities");
    capability_plan(&config, &machine).expect("capability plan")
}

