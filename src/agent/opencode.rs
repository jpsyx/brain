//! Fail-fast OpenCode selection stub.

use std::path::PathBuf;

use crate::agent::{
    AgentError, AgentFrontend, AgentKind, AgentSession, CompletionStrategy, InputSequence,
    LaunchRequest, LaunchSpec,
};

pub(crate) const DEFAULT_COMMAND: &str = "opencode";

/// Constructible OpenCode adapter whose operational surface is intentionally unavailable.
pub struct OpenCodeFrontend;

impl OpenCodeFrontend {
    /// Construct the stub without inspecting sessions, hooks, or the executable.
    #[must_use]
    pub fn new(_command: impl Into<String>) -> Self {
        Self
    }

    fn unsupported<T>() -> Result<T, AgentError> {
        Err(AgentError::UnsupportedFrontend(AgentKind::OpenCode))
    }
}

impl AgentFrontend for OpenCodeFrontend {
    fn kind(&self) -> AgentKind {
        AgentKind::OpenCode
    }

    fn ensure_available(&self) -> Result<(), AgentError> {
        Self::unsupported()
    }

    fn launch_spec(&self, _request: &LaunchRequest) -> Result<LaunchSpec, AgentError> {
        Self::unsupported()
    }

    fn submit_input(&self) -> Result<InputSequence, AgentError> {
        Self::unsupported()
    }

    fn queue_input(&self) -> Result<InputSequence, AgentError> {
        Self::unsupported()
    }

    fn new_session_input(&self) -> Result<InputSequence, AgentError> {
        Self::unsupported()
    }

    fn completion_strategy(&self) -> Result<CompletionStrategy, AgentError> {
        Self::unsupported()
    }

    fn transcript(&self, _session: &AgentSession) -> Result<Option<PathBuf>, AgentError> {
        Self::unsupported()
    }

    fn resume_candidate_exists(&self, _session: &AgentSession) -> Result<bool, AgentError> {
        Self::unsupported()
    }

    fn response_id(&self, _session: &AgentSession) -> Result<String, AgentError> {
        Self::unsupported()
    }

    fn can_resume_response_session(&self) -> Result<bool, AgentError> {
        Self::unsupported()
    }
}
