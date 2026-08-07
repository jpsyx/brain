use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ControllerEvent {
    SubmitNow,
    QueueAfterActiveTurn,
    QueueDelivered,
    StartNewSession,
    ScrollUp(usize),
    ScrollDown(usize),
    Shutdown,
}

#[derive(Clone, Default)]
pub(crate) struct ControllerRecording(Arc<Mutex<Vec<ControllerEvent>>>);

impl ControllerRecording {
    fn record(&self, event: ControllerEvent) {
        self.0.lock().expect("controller recording").push(event);
    }

    pub(crate) fn events(&self) -> Vec<ControllerEvent> {
        self.0.lock().expect("controller recording").clone()
    }
}

struct RecordingFrontend {
    kind: AgentKind,
    recording: ControllerRecording,
}

impl AgentFrontend for RecordingFrontend {
    fn kind(&self) -> AgentKind {
        self.kind
    }

    fn launch_spec(&self, request: &LaunchRequest) -> Result<LaunchSpec, AgentError> {
        Ok(LaunchSpec::new(
            "recording-agent",
            request.workspace().root().to_path_buf(),
            Vec::new(),
            HookMetadata::none(),
        ))
    }

    fn input_for(
        &self,
        action: crate::agent::AgentAction<'_>,
    ) -> Result<InputSequence, AgentError> {
        Ok(match action {
            crate::agent::AgentAction::TypeText(text) => InputSequence::text(text),
            crate::agent::AgentAction::SubmitNow => {
                self.recording.record(ControllerEvent::SubmitNow);
                InputSequence::bytes(b"\r")
            }
            crate::agent::AgentAction::FollowUpAfterActiveTurn(text) => {
                self.recording.record(ControllerEvent::QueueAfterActiveTurn);
                InputSequence::text_with_suffix(text, b"\x1dqueue")
            }
            crate::agent::AgentAction::StartNewSession => {
                self.recording.record(ControllerEvent::StartNewSession);
                InputSequence::bytes(b"/new\r")
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

    fn scroll_up(&mut self, rows: usize) {
        self.recording.record(ControllerEvent::ScrollUp(rows));
    }

    fn scroll_down(&mut self, rows: usize) {
        self.recording.record(ControllerEvent::ScrollDown(rows));
    }

    fn terminal_rows(&self) -> u16 {
        40
    }
}

#[derive(Clone, Default)]
pub(crate) struct LaunchRecording(pub(crate) Arc<Mutex<Vec<LaunchSpec>>>);

pub(crate) struct LaunchRecordingTransport {
    pub(crate) recording: LaunchRecording,
    pub(crate) alive: bool,
}

#[derive(Clone, Default)]
pub(crate) struct TransportRecording(Arc<Mutex<TransportState>>);

#[derive(Default)]
struct TransportState {
    launch_specs: Vec<LaunchSpec>,
    inputs: Vec<Vec<u8>>,
    shutdowns: usize,
    alive: bool,
}

impl TransportRecording {
    pub(crate) fn transport(&self) -> Box<dyn AgentTransport> {
        Box::new(ObservedTransport {
            recording: self.clone(),
        })
    }

    pub(crate) fn launch_specs(&self) -> Vec<LaunchSpec> {
        self.0
            .lock()
            .expect("transport recording")
            .launch_specs
            .clone()
    }

    pub(crate) fn inputs(&self) -> Vec<Vec<u8>> {
        self.0.lock().expect("transport recording").inputs.clone()
    }

    pub(crate) fn shutdowns(&self) -> usize {
        self.0.lock().expect("transport recording").shutdowns
    }

    pub(crate) fn set_alive(&self, alive: bool) {
        self.0.lock().expect("transport recording").alive = alive;
    }
}

struct ObservedTransport {
    recording: TransportRecording,
}

impl AgentTransport for ObservedTransport {
    fn spawn(&mut self, spec: &LaunchSpec) -> Result<(), AgentError> {
        {
            let mut state = self.recording.0.lock().expect("transport recording");
            state.launch_specs.push(spec.clone());
            state.alive = true;
        }
        Ok(())
    }

    fn send(&mut self, input: InputSequence) -> Result<(), AgentError> {
        self.recording
            .0
            .lock()
            .expect("transport recording")
            .inputs
            .push(input.into_bytes());
        Ok(())
    }

    fn snapshot(&self) -> String {
        String::new()
    }

    fn is_alive(&self) -> bool {
        self.recording.0.lock().expect("transport recording").alive
    }

    fn shutdown(&mut self) {
        let mut state = self.recording.0.lock().expect("transport recording");
        state.shutdowns += 1;
        state.alive = false;
    }
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

pub(crate) fn recording_controller(
    app: &App<'_>,
    alive: bool,
    snapshot: &str,
) -> (AgentController, ControllerRecording) {
    recording_controller_for_actor(app, app.interactive_actor.clone(), alive, snapshot)
}

pub(crate) fn recording_controller_for_actor(
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
            kind: app.agent_kind,
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
