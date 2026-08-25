use std::{path::Path, sync::{Arc, Mutex}};

use crate::{
    access::{AccessMode, MachineCapabilityEnvironment, capability_plan},
    actor::ActorContext,
    agent::{
        AccessPolicy, AgentAction, AgentController, AgentError, AgentFrontend, AgentKind,
        AgentSession, AgentTransport, CompletionStrategy, HookMetadata, InputSequence,
        LaunchRequest, LaunchSpec, SessionPlan,
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
    Rollback,
    FrontendNewSession,
    TransportNewSession(InputSequence),
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
    available: bool,
    command: String,
}

impl AgentFrontend for RecordingFrontend {
    fn kind(&self) -> AgentKind {
        AgentKind::Claude
    }

    fn ensure_available(&self) -> Result<(), AgentError> {
        if self.available {
            Ok(())
        } else {
            Err(AgentError::Frontend("compatibility probe failed".to_owned()))
        }
    }

    fn launch_spec(&self, request: &LaunchRequest) -> Result<LaunchSpec, AgentError> {
        self.recording
            .record(Event::Launch(request.session_plan().clone()));
        Ok(LaunchSpec::new(
            self.command.clone(),
            request.workspace().root().to_path_buf(),
            Vec::new(),
            HookMetadata::none(),
        ))
    }

    fn rollback_launch(&self, _request: &LaunchRequest) -> Result<(), AgentError> {
        self.recording.record(Event::Rollback);
        Ok(())
    }

    fn input_for(&self, action: AgentAction<'_>) -> Result<InputSequence, AgentError> {
        Ok(match action {
            AgentAction::TypeText(text) => InputSequence::text(text),
            AgentAction::SubmitNow => {
                self.recording.record(Event::Submit);
                InputSequence::bytes(b"\x1dsubmit")
            }
            AgentAction::FollowUpAfterActiveTurn(text) => {
                InputSequence::text_then_key(text, QUEUE_MARKER)
            }
            AgentAction::StartNewSession => {
                self.recording.record(Event::FrontendNewSession);
                InputSequence::bytes(NEW_SESSION_MARKER)
            }
        })
    }

    fn completion_strategy(&self) -> Result<CompletionStrategy, AgentError> {
        Ok(CompletionStrategy::Hook)
    }

    fn resume_candidate_exists(&self, _session: &AgentSession) -> Result<bool, AgentError> {
        Ok(true)
    }

    fn response_id(&self, session: &AgentSession) -> Result<String, AgentError> {
        Ok(session.as_str().to_owned())
    }

    fn can_resume_response_session(&self, _session: &AgentSession) -> Result<bool, AgentError> {
        Ok(true)
    }
}

struct RecordingTransport {
    recording: Recording,
    pending_text: Option<String>,
}

struct FailingSpawnTransport {
    recording: Recording,
}

impl AgentTransport for FailingSpawnTransport {
    fn spawn(&mut self, _spec: &LaunchSpec) -> Result<(), AgentError> {
        self.recording.record(Event::Spawn);
        Err(AgentError::Transport("spawn failed".to_owned()))
    }

    fn send(&mut self, _input: InputSequence) -> Result<(), AgentError> {
        Ok(())
    }

    fn snapshot(&self) -> String {
        String::new()
    }

    fn is_alive(&self) -> bool {
        false
    }

    fn shutdown(&mut self) {}
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

        let bytes = input.flattened();
        if let Some(text) = bytes.strip_suffix(QUEUE_MARKER) {
            let text = if text.is_empty() {
                self.pending_text.take().unwrap_or_default()
            } else {
                crate::agent::input::paste_payload(text)
            };
            self.recording.record(Event::Queue(text));
        } else if bytes != b"\x1dsubmit" {
            let text = crate::agent::input::paste_payload(&bytes);
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
            available: true,
            command: "recording-agent".to_owned(),
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
