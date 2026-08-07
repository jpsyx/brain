//! OpenCode translation behind the frontend-neutral agent facade.

use std::path::PathBuf;

use crate::agent::{
    AgentError, AgentFrontend, AgentKind, AgentSession, CompletionStrategy, HookMetadata,
    InputSequence, LaunchRequest, LaunchSpec, SessionPlan,
    frontend::{launch_environment, shell_quote},
};

pub(crate) const DEFAULT_COMMAND: &str = "opencode";

/// OpenCode command, input, completion, and session conventions.
pub struct OpenCodeFrontend {
    command: String,
}

impl OpenCodeFrontend {
    /// Construct an OpenCode adapter from its effective launch command.
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
        let mut parts = vec![
            command.trim().to_owned(),
            "--agent".to_owned(),
            "brain".to_owned(),
        ];
        if let SessionPlan::Resume(session) = plan {
            parts.push("--session".to_owned());
            parts.push(shell_quote(session.as_str()));
        }
        if let Some(prompt) = prompt.map(str::trim).filter(|prompt| !prompt.is_empty()) {
            parts.push("--prompt".to_owned());
            parts.push(shell_quote(prompt));
        }
        parts.join(" ")
    }
}

impl AgentFrontend for OpenCodeFrontend {
    fn kind(&self) -> AgentKind {
        AgentKind::OpenCode
    }

    fn launch_spec(&self, request: &LaunchRequest) -> Result<LaunchSpec, AgentError> {
        let brain_prompt = request.access_policy().boundary_prompt().unwrap_or(
            "You are the Brain agent. Follow the user's request and report completion clearly.",
        );
        let config = serde_json::json!({
            "agent": {
                "brain": {
                    "mode": "primary",
                    "prompt": brain_prompt,
                }
            },
            "default_agent": "brain",
        });
        let mut environment = launch_environment(request, self.kind());
        environment.retain(|(name, _)| name != "OPENCODE_CONFIG_CONTENT");
        environment.push((
            "OPENCODE_CONFIG_CONTENT".to_owned(),
            serde_json::to_string(&config).map_err(|error| {
                AgentError::Frontend(format!("serialize OpenCode config: {error}"))
            })?,
        ));
        let capabilities = request
            .access_policy()
            .capability_plan()
            .map_or_else(Default::default, |plan| {
                plan.enforcement_report(crate::access::EnforcementEvidence::advisory_only())
            });
        Ok(LaunchSpec::new(
            Self::command_for(
                &self.command,
                request.session_plan(),
                request.initial_prompt(),
            ),
            request.workspace().root().to_path_buf(),
            environment,
            HookMetadata::none(),
        )
        .with_capabilities(capabilities))
    }

    fn submit_input(&self) -> Result<InputSequence, AgentError> {
        Ok(InputSequence::bytes(b"\r"))
    }

    fn queue_input(&self) -> Result<InputSequence, AgentError> {
        Ok(InputSequence::bytes(b"\r"))
    }

    fn new_session_input(&self) -> Result<InputSequence, AgentError> {
        Ok(InputSequence::bytes(b"/new\r"))
    }

    fn completion_strategy(&self) -> Result<CompletionStrategy, AgentError> {
        Ok(CompletionStrategy::Hook)
    }

    fn transcript(&self, _session: &AgentSession) -> Result<Option<PathBuf>, AgentError> {
        Ok(None)
    }

    fn resume_candidate_exists(&self, _session: &AgentSession) -> Result<bool, AgentError> {
        Ok(true)
    }

    fn response_id(&self, session: &AgentSession) -> Result<String, AgentError> {
        let identity = format!("brain://opencode/response/{}", session.as_str());
        Ok(uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, identity.as_bytes()).to_string())
    }

    fn can_resume_response_session(&self) -> Result<bool, AgentError> {
        Ok(true)
    }
}
