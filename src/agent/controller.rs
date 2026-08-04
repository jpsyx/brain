//! The semantic facade shared by TUI and receiver agent callers.

use std::sync::{Arc, RwLock};

use crate::{
    actor::ActorContext,
    agent::{
        AgentError, AgentFrontend, AgentSession, CompletionStrategy, InputSequence, LaunchRequest,
        LaunchSpec,
    },
    workspace::WorkspaceContext,
};

/// Transport implementation owned by a live agent panel.
pub trait AgentTransport: Send {
    /// Start a child from a complete frontend launch spec.
    fn spawn(&mut self, spec: &LaunchSpec) -> Result<(), AgentError>;

    /// Deliver an input sequence to the active frontend.
    fn send(&mut self, input: InputSequence) -> Result<(), AgentError>;

    /// Return the transport's current visible output.
    fn snapshot(&self) -> String;

    /// Whether the frontend child is still running.
    fn is_alive(&self) -> bool;

    /// End the active frontend child.
    fn shutdown(&mut self);

    /// Terminal screen exposed by transports that render an interactive pane.
    fn terminal_screen(&self) -> Option<Arc<RwLock<vt100::Parser>>> {
        None
    }

    /// Resize an interactive terminal transport.
    fn resize(&mut self, _rows: u16, _cols: u16) {}

    /// Scroll an interactive terminal transport up through retained output.
    fn scroll_up(&mut self, _rows: usize) {}

    /// Scroll an interactive terminal transport down toward live output.
    fn scroll_down(&mut self, _rows: usize) {}

    /// Snap an interactive terminal transport to its live output.
    fn scroll_to_bottom(&mut self) {}

    /// Current interactive terminal row count.
    fn terminal_rows(&self) -> u16 {
        0
    }
}

/// Frontend-neutral semantic control of one live agent.
pub struct AgentController {
    workspace: Arc<WorkspaceContext>,
    actor: ActorContext,
    frontend: Box<dyn AgentFrontend>,
    transport: Box<dyn AgentTransport>,
    pending_input: Option<PendingInput>,
    shutdown: bool,
}

struct PendingInput {
    ticks: u8,
    input: InputSequence,
}

const QUEUED_INPUT_DELAY_TICKS: u8 = 2;

impl AgentController {
    /// Construct a controller bound to one workspace, actor, frontend, and transport.
    #[must_use]
    pub fn new(
        workspace: Arc<WorkspaceContext>,
        actor: ActorContext,
        frontend: Box<dyn AgentFrontend>,
        transport: Box<dyn AgentTransport>,
    ) -> Self {
        Self {
            workspace,
            actor,
            frontend,
            transport,
            pending_input: None,
            shutdown: false,
        }
    }

    /// Build a frontend launch spec and start it through the transport.
    ///
    /// # Errors
    ///
    /// Returns an error when the request context differs from this controller,
    /// the frontend rejects the request, or the transport cannot spawn it.
    pub fn launch(&mut self, request: &LaunchRequest) -> Result<(), AgentError> {
        if request.workspace().id() != self.workspace.id() || request.actor() != &self.actor {
            return Err(AgentError::ContextMismatch);
        }
        let spec = self.frontend.launch_spec(request)?;
        self.transport.spawn(&spec)
    }

    /// Type literal text into the active agent without submitting it.
    ///
    /// # Errors
    ///
    /// Returns an error when the transport cannot deliver the text.
    pub fn type_text(&mut self, text: &str) -> Result<(), AgentError> {
        self.transport.send(InputSequence::text(text))
    }

    /// Forward frontend-neutral terminal bytes that are not a semantic submit.
    ///
    /// # Errors
    ///
    /// Returns an error when the transport cannot deliver the bytes.
    pub fn forward_terminal_input(&mut self, bytes: Vec<u8>) -> Result<(), AgentError> {
        self.transport.send(InputSequence::bytes(bytes))
    }

    /// Immediately submit the active agent input.
    ///
    /// # Errors
    ///
    /// Returns an error when the transport cannot deliver the frontend input.
    pub fn submit_now(&mut self) -> Result<(), AgentError> {
        self.transport.send(self.frontend.submit_input())
    }

    /// Queue non-blank text after the frontend's active turn.
    ///
    /// # Errors
    ///
    /// Returns [`AgentError::EmptyInput`] for blank text or an error when the
    /// transport cannot deliver the frontend input.
    pub fn queue_after_active_turn(&mut self, text: &str) -> Result<(), AgentError> {
        if text.trim().is_empty() {
            return Err(AgentError::EmptyInput);
        }
        self.transport.send(InputSequence::text(text))?;
        self.pending_input = Some(PendingInput {
            ticks: QUEUED_INPUT_DELAY_TICKS,
            input: self.frontend.queue_input(),
        });
        Ok(())
    }

    /// Advance controller-owned delayed input by one event-loop tick.
    ///
    /// # Errors
    ///
    /// Returns an error when a pending frontend input becomes due and the
    /// transport cannot deliver it.
    pub fn tick(&mut self) -> Result<(), AgentError> {
        let Some(pending) = self.pending_input.as_mut() else {
            return Ok(());
        };
        pending.ticks = pending.ticks.saturating_sub(1);
        if pending.ticks > 0 {
            return Ok(());
        }
        let pending = self.pending_input.take().expect("pending input exists");
        self.transport.send(pending.input)
    }

    /// Request a new session through the selected frontend.
    ///
    /// # Errors
    ///
    /// Returns an error when the transport cannot deliver the frontend input.
    pub fn start_new_session(&mut self) -> Result<(), AgentError> {
        self.transport.send(self.frontend.new_session_input())
    }

    /// The selected frontend's completion mechanism.
    #[must_use]
    pub fn completion_strategy(&self) -> CompletionStrategy {
        self.frontend.completion_strategy()
    }

    /// Look up a transcript through the selected frontend.
    #[must_use]
    pub fn transcript(&self, session: &AgentSession) -> Option<std::path::PathBuf> {
        self.frontend.transcript(session)
    }

    /// Snapshot the transport's visible output.
    #[must_use]
    pub fn snapshot(&self) -> String {
        self.transport.snapshot()
    }

    /// Whether the selected frontend child is still running.
    #[must_use]
    pub fn is_alive(&self) -> bool {
        self.transport.is_alive()
    }

    /// Shut down the active child through its transport.
    pub fn shutdown(&mut self) {
        if !self.shutdown {
            self.pending_input = None;
            self.transport.shutdown();
            self.shutdown = true;
        }
    }

    /// The selected frontend kind.
    #[must_use]
    pub fn kind(&self) -> crate::agent::AgentKind {
        self.frontend.kind()
    }

    /// Immutable initiating actor and channel bound to this controller.
    #[must_use]
    pub const fn actor(&self) -> &ActorContext {
        &self.actor
    }

    /// Whether a known frontend session can be resumed.
    #[must_use]
    pub fn resume_candidate_exists(&self, session: &AgentSession) -> bool {
        self.frontend.resume_candidate_exists(session)
    }

    /// Stable response artifact identity for a frontend session.
    #[must_use]
    pub fn response_id(&self, session: &AgentSession) -> String {
        self.frontend.response_id(session)
    }

    /// Whether the selected frontend can resume a completed receiver session.
    #[must_use]
    pub fn can_resume_response_session(&self) -> bool {
        self.frontend.can_resume_response_session()
    }

    /// Terminal screen for rendering an interactive transport.
    #[must_use]
    pub fn terminal_screen(&self) -> Option<Arc<RwLock<vt100::Parser>>> {
        self.transport.terminal_screen()
    }

    /// Resize the interactive transport.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.transport.resize(rows, cols);
    }

    /// Scroll the interactive transport up.
    pub fn scroll_up(&mut self, rows: usize) {
        self.transport.scroll_up(rows);
    }

    /// Scroll the interactive transport down.
    pub fn scroll_down(&mut self, rows: usize) {
        self.transport.scroll_down(rows);
    }

    /// Snap the interactive transport to live output.
    pub fn scroll_to_bottom(&mut self) {
        self.transport.scroll_to_bottom();
    }

    /// Current interactive terminal row count.
    #[must_use]
    pub fn terminal_rows(&self) -> u16 {
        self.transport.terminal_rows()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        sync::{Arc, Mutex},
    };

    use crate::{
        actor::ActorContext,
        agent::{
            AccessPolicy, AgentController, AgentError, AgentFrontend, AgentKind, AgentSession,
            AgentTransport, CompletionStrategy, HookMetadata, InputSequence, LaunchRequest,
            LaunchSpec, SessionPlan,
        },
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

        fn submit_input(&self) -> InputSequence {
            self.recording.record(Event::Submit);
            InputSequence::bytes(b"\x1dsubmit")
        }

        fn queue_input(&self) -> InputSequence {
            InputSequence::bytes(QUEUE_MARKER)
        }

        fn new_session_input(&self) -> InputSequence {
            self.recording.record(Event::FrontendNewSession);
            InputSequence::bytes(NEW_SESSION_MARKER)
        }

        fn completion_strategy(&self) -> CompletionStrategy {
            CompletionStrategy::Hook
        }

        fn transcript(&self, session: &AgentSession) -> Option<PathBuf> {
            self.recording.record(Event::Transcript(session.clone()));
            Some(PathBuf::from("/transcripts").join(session.as_str()))
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

    #[test]
    fn semantic_operations_are_forwarded_without_callers_constructing_keystrokes() {
        let (mut controller, recording, _, _) = controller();

        controller.type_text("hello").expect("type text");
        controller.submit_now().expect("submit");
        controller
            .queue_after_active_turn("next")
            .expect("queue after turn");

        assert_eq!(
            recording.events(),
            vec![
                Event::Type("hello".to_owned()),
                Event::Submit,
                Event::Type("next".to_owned()),
            ]
        );

        controller.tick().expect("first delayed-input tick");
        controller.tick().expect("second delayed-input tick");

        assert_eq!(
            recording.events(),
            vec![
                Event::Type("hello".to_owned()),
                Event::Submit,
                Event::Type("next".to_owned()),
                Event::Queue("next".to_owned()),
            ]
        );
    }

    #[test]
    fn launch_preserves_fresh_and_resume_session_selection() {
        let (mut controller, recording, workspace, actor) = controller();
        let fresh = SessionPlan::fresh(AgentSession::new("fresh-1").expect("session"));
        let resume = SessionPlan::resume(AgentSession::new("resume-1").expect("session"));

        controller
            .launch(&request(
                Arc::clone(&workspace),
                actor.clone(),
                fresh.clone(),
            ))
            .expect("fresh launch");
        controller
            .launch(&request(workspace, actor, resume.clone()))
            .expect("resume launch");

        assert_eq!(
            recording.events(),
            vec![
                Event::Launch(fresh),
                Event::Spawn,
                Event::Launch(resume),
                Event::Spawn,
            ]
        );
    }

    #[test]
    fn completion_strategy_and_transcript_lookup_delegate_to_the_frontend() {
        let (controller, recording, _, _) = controller();
        let session = AgentSession::new("session-1").expect("session");

        assert_eq!(controller.completion_strategy(), CompletionStrategy::Hook);
        assert_eq!(
            controller.transcript(&session),
            Some(PathBuf::from("/transcripts/session-1"))
        );
        assert_eq!(recording.events(), vec![Event::Transcript(session)]);
    }

    #[test]
    fn shutdown_delegates_once_to_the_transport() {
        let (mut controller, recording, _, _) = controller();

        controller.shutdown();

        assert_eq!(recording.events(), vec![Event::Shutdown]);
    }

    #[test]
    fn queueing_rejects_empty_text_before_calling_the_frontend_or_transport() {
        let (mut controller, recording, _, _) = controller();

        assert_eq!(
            controller.queue_after_active_turn("   "),
            Err(AgentError::EmptyInput)
        );

        assert!(recording.events().is_empty());
    }

    #[test]
    fn starting_a_new_session_delegates_to_the_frontend() {
        let (mut controller, recording, _, _) = controller();

        controller.start_new_session().expect("new session");

        assert_eq!(
            recording.events(),
            vec![
                Event::FrontendNewSession,
                Event::TransportNewSession(InputSequence::bytes(NEW_SESSION_MARKER)),
            ]
        );
    }
}
