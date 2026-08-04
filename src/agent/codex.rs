//! OpenAI Codex translation behind the frontend-neutral agent facade.

use std::path::PathBuf;

use crate::agent::{
    AgentError, AgentFrontend, AgentKind, AgentSession, CompletionStrategy, HookMetadata,
    InputSequence, LaunchRequest, LaunchSpec, SessionPlan,
    frontend::{launch_environment, shell_quote},
};

pub(crate) const DEFAULT_COMMAND: &str = "codex";

/// Codex command, input, completion, and transcript conventions.
pub struct CodexFrontend {
    command: String,
}

impl CodexFrontend {
    /// Construct a Codex adapter from its effective launch command.
    #[must_use]
    pub fn new(command: impl Into<String>) -> Self {
        let command = command.into();
        let command = command.trim();
        Self {
            command: if command.is_empty() {
                DEFAULT_COMMAND.to_owned()
            } else {
                command.to_owned()
            },
        }
    }

    pub(super) fn command_for(command: &str, plan: &SessionPlan, prompt: Option<&str>) -> String {
        let mut parts = vec![command.trim().to_owned()];
        if let SessionPlan::Resume(session) = plan {
            parts.push("resume".to_owned());
            parts.push(shell_quote(session.as_str()));
        }
        if let Some(prompt) = prompt {
            let prompt = prompt.trim();
            if !prompt.is_empty() {
                parts.push(shell_quote(prompt));
            }
        }
        parts.join(" ")
    }
}

impl AgentFrontend for CodexFrontend {
    fn kind(&self) -> AgentKind {
        AgentKind::Codex
    }

    fn launch_spec(&self, request: &LaunchRequest) -> Result<LaunchSpec, AgentError> {
        Ok(LaunchSpec::new(
            Self::command_for(
                &self.command,
                request.session_plan(),
                request.initial_prompt(),
            ),
            request.workspace().root().to_path_buf(),
            launch_environment(request, self.kind()),
            HookMetadata::none(),
        ))
    }

    fn submit_input(&self) -> InputSequence {
        InputSequence::bytes(b"\r")
    }

    fn queue_input(&self) -> InputSequence {
        InputSequence::bytes(b"\t")
    }

    fn new_session_input(&self) -> InputSequence {
        InputSequence::bytes(b"/new\t")
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
        let identity = format!("brain://codex/response/{}", session.as_str());
        uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, identity.as_bytes()).to_string()
    }

    fn registers_fresh_session(&self) -> bool {
        false
    }

    fn can_resume_response_session(&self) -> bool {
        false
    }
}
