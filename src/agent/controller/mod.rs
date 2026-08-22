//! The semantic facade shared by TUI and receiver agent callers.

use std::sync::{Arc, RwLock};

use crate::{
    actor::ActorContext,
    agent::{
        AgentAction, AgentError, AgentFrontend, AgentSession, CompletionStrategy, InputSequence,
        LaunchRequest, LaunchSpec,
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
    shutdown: bool,
}

impl AgentController {
    /// Construct a controller from the selected configured frontend.
    #[must_use]
    pub fn configured(
        command: &crate::workspace::CommandContext,
        kind: crate::agent::AgentKind,
        actor: ActorContext,
        transport: Box<dyn AgentTransport>,
    ) -> Self {
        Self::new(
            Arc::clone(&command.workspace),
            actor,
            crate::agent::configured_frontend(command, kind),
            transport,
        )
    }

    pub(crate) fn configured_with_command(
        command: &crate::workspace::CommandContext,
        kind: crate::agent::AgentKind,
        configured_command: String,
        actor: ActorContext,
        transport: Box<dyn AgentTransport>,
    ) -> Self {
        Self::new(
            Arc::clone(&command.workspace),
            actor,
            crate::agent::configured_frontend_with_command(
                &command.workspace,
                kind,
                configured_command,
            ),
            transport,
        )
    }

    /// Construct a controller for an already resolved workspace with an explicit
    /// frontend command.
    #[must_use]
    pub fn for_workspace_with_command(
        workspace: Arc<WorkspaceContext>,
        kind: crate::agent::AgentKind,
        configured_command: String,
        actor: ActorContext,
        transport: Box<dyn AgentTransport>,
    ) -> Self {
        let frontend =
            crate::agent::configured_frontend_with_command(&workspace, kind, configured_command);
        Self::new(workspace, actor, frontend, transport)
    }

    /// Construct a controller bound to one workspace, actor, frontend, and transport.
    #[must_use]
    pub(crate) fn new(
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
            shutdown: false,
        }
    }

    /// Check that the selected frontend can service facade operations.
    pub fn ensure_available(&self) -> Result<(), AgentError> {
        self.frontend.ensure_available()
    }

    /// Build a frontend launch spec and start it through the transport.
    ///
    /// # Errors
    ///
    /// Returns an error when the request context differs from this controller,
    /// the frontend rejects the request, or the transport cannot spawn it.
    pub fn launch(&mut self, request: &LaunchRequest) -> Result<(), AgentError> {
        self.frontend.ensure_available()?;
        if request.workspace().id() != self.workspace.id() || request.actor() != &self.actor {
            return Err(AgentError::ContextMismatch);
        }
        if !request
            .access_policy()
            .matches_capability_context(self.workspace.id())
        {
            return Err(AgentError::ContextMismatch);
        }
        let spec = self.frontend.launch_spec(request)?;
        if let Err(spawn_error) = self.transport.spawn(&spec) {
            if let Err(rollback_error) = self.frontend.rollback_launch(request) {
                return Err(AgentError::Frontend(format!(
                    "{spawn_error}; launch rollback failed: {rollback_error}"
                )));
            }
            return Err(spawn_error);
        }
        Ok(())
    }

    /// Type literal text into the active agent without submitting it.
    ///
    /// # Errors
    ///
    /// Returns an error when the transport cannot deliver the text.
    pub fn type_text(&mut self, text: &str) -> Result<(), AgentError> {
        self.frontend.ensure_available()?;
        self.transport
            .send(self.frontend.input_for(AgentAction::TypeText(text))?)
    }

    /// Forward frontend-neutral terminal bytes that are not a semantic submit.
    ///
    /// # Errors
    ///
    /// Returns an error when the transport cannot deliver the bytes.
    pub fn forward_terminal_input(&mut self, bytes: Vec<u8>) -> Result<(), AgentError> {
        self.frontend.ensure_available()?;
        self.transport.send(InputSequence::bytes(bytes))
    }

    /// Immediately submit the active agent input.
    ///
    /// # Errors
    ///
    /// Returns an error when the transport cannot deliver the frontend input.
    pub fn submit_now(&mut self) -> Result<(), AgentError> {
        self.frontend.ensure_available()?;
        self.transport
            .send(self.frontend.input_for(AgentAction::SubmitNow)?)
    }

    /// Queue non-blank text after the frontend's active turn.
    ///
    /// # Errors
    ///
    /// Returns [`AgentError::EmptyInput`] for blank text or an error when the
    /// transport cannot deliver the frontend input.
    pub fn queue_after_active_turn(&mut self, text: &str) -> Result<(), AgentError> {
        self.frontend.ensure_available()?;
        if text.trim().is_empty() {
            return Err(AgentError::EmptyInput);
        }
        let sequence = self
            .frontend
            .input_for(AgentAction::FollowUpAfterActiveTurn(text))?;
        // A follow-up is the only prompt brain types rather than passes as a
        // launch argument, and the only one that has ever gone unsubmitted, so
        // the exact wire form — including how the writes are paced — is
        // recorded before it leaves.
        let bytes = sequence.flattened();
        crate::logging::log(format!(
            "agent follow-up delivery: {} bytes in {} write(s) head={:02x?} tail={:02x?} paste_opened={} paste_closed={}",
            bytes.len(),
            sequence.writes().len(),
            &bytes[..bytes.len().min(8)],
            &bytes[bytes.len().saturating_sub(8)..],
            bytes.starts_with(b"\x1b[200~"),
            bytes.windows(6).any(|window| window == b"\x1b[201~"),
        ));
        self.transport.send(sequence)
    }

    /// Request a new session through the selected frontend.
    ///
    /// # Errors
    ///
    /// Returns an error when the transport cannot deliver the frontend input.
    pub fn start_new_session(&mut self) -> Result<(), AgentError> {
        self.frontend.ensure_available()?;
        self.transport
            .send(self.frontend.input_for(AgentAction::StartNewSession)?)
    }

    /// The selected frontend's completion mechanism.
    pub fn completion_strategy(&self) -> Result<CompletionStrategy, AgentError> {
        self.frontend.ensure_available()?;
        self.frontend.completion_strategy()
    }

    /// Snapshot the transport's visible output.
    pub fn snapshot(&self) -> Result<String, AgentError> {
        self.frontend.ensure_available()?;
        Ok(self.transport.snapshot())
    }

    /// Whether the selected frontend child is still running.
    pub fn is_alive(&self) -> Result<bool, AgentError> {
        self.frontend.ensure_available()?;
        Ok(self.transport.is_alive())
    }

    /// Shut down the active child through its transport.
    pub fn shutdown(&mut self) -> Result<(), AgentError> {
        self.frontend.ensure_available()?;
        if !self.shutdown {
            self.transport.shutdown();
            self.shutdown = true;
        }
        Ok(())
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
    pub fn resume_candidate_exists(&self, session: &AgentSession) -> Result<bool, AgentError> {
        self.frontend.ensure_available()?;
        self.frontend.resume_candidate_exists(session)
    }

    /// Stable response artifact identity for a frontend session.
    pub fn response_id(&self, session: &AgentSession) -> Result<String, AgentError> {
        self.frontend.ensure_available()?;
        self.frontend.response_id(session)
    }

    /// Whether the selected frontend can resume a completed receiver session.
    pub fn can_resume_response_session(&self, session: &AgentSession) -> Result<bool, AgentError> {
        self.frontend.ensure_available()?;
        self.frontend.can_resume_response_session(session)
    }

    /// Terminal screen for rendering an interactive transport.
    pub fn terminal_screen(&self) -> Result<Option<Arc<RwLock<vt100::Parser>>>, AgentError> {
        self.frontend.ensure_available()?;
        Ok(self.transport.terminal_screen())
    }

    /// Resize the interactive transport.
    pub fn resize(&mut self, rows: u16, cols: u16) -> Result<(), AgentError> {
        self.frontend.ensure_available()?;
        self.transport.resize(rows, cols);
        Ok(())
    }

    /// Scroll the interactive transport up.
    pub fn scroll_up(&mut self, rows: usize) -> Result<(), AgentError> {
        self.frontend.ensure_available()?;
        self.transport.scroll_up(rows);
        Ok(())
    }

    /// Scroll the interactive transport down.
    pub fn scroll_down(&mut self, rows: usize) -> Result<(), AgentError> {
        self.frontend.ensure_available()?;
        self.transport.scroll_down(rows);
        Ok(())
    }

    /// Snap the interactive transport to live output.
    pub fn scroll_to_bottom(&mut self) -> Result<(), AgentError> {
        self.frontend.ensure_available()?;
        self.transport.scroll_to_bottom();
        Ok(())
    }

    /// Current interactive terminal row count.
    pub fn terminal_rows(&self) -> Result<u16, AgentError> {
        self.frontend.ensure_available()?;
        Ok(self.transport.terminal_rows())
    }
}

#[cfg(test)]
mod tests;
