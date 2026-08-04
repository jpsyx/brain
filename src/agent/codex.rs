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
        Self::command_for_with_policy(command, plan, prompt, None, &[])
    }

    fn command_for_with_policy(
        command: &str,
        plan: &SessionPlan,
        prompt: Option<&str>,
        policy: Option<&str>,
        capability_overrides: &[String],
    ) -> String {
        let mut parts = vec![command.trim().to_owned()];
        if let Some(policy) = policy {
            let policy = serde_json::to_string(policy)
                .expect("serializing a Rust string as JSON cannot fail");
            parts.push("-c".to_owned());
            parts.push(shell_quote(&format!("developer_instructions={policy}")));
        }
        for capability_override in capability_overrides {
            parts.push("-c".to_owned());
            parts.push(shell_quote(capability_override));
        }
        if let SessionPlan::Resume(session) = plan {
            parts.push("resume".to_owned());
            parts.push(shell_quote(session.as_str()));
        }
        if let Some(prompt) = prompt {
            let prompt = prompt.trim();
            if !prompt.is_empty() {
                parts.push("--".to_owned());
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
        let capability_plan = request.access_policy().capability_plan();
        if request.access_policy().mode() == crate::access::AccessMode::Unrestricted {
            crate::access::cleanup_workspace_capabilities(request.workspace())
                .map_err(|error| AgentError::Frontend(error.to_string()))?;
        } else if capability_plan.is_some() {
            crate::access::prepare_workspace_capabilities(request.workspace())
                .and_then(|()| crate::access::cleanup_claude_runtime_artifacts(request.workspace()))
                .map_err(|error| AgentError::Frontend(error.to_string()))?;
        }
        if let Some(plan) = capability_plan.filter(|plan| !plan.skills.uses_global_configuration())
        {
            crate::skills::render_workspace_capabilities(
                request.workspace(),
                request.actor(),
                plan,
            )
            .map_err(|error| AgentError::Frontend(error.to_string()))?;
        }
        let capability_launch = capability_plan
            .filter(|plan| !plan.mcps.uses_global_configuration())
            .map(|plan| crate::access::codex_mcp_launch(request.workspace(), plan))
            .transpose()
            .map_err(|error| AgentError::Frontend(error.to_string()))?;
        let overrides = capability_launch
            .as_ref()
            .map_or(&[][..], |launch| launch.overrides.as_slice());
        let mut environment = launch_environment(request, self.kind());
        if let Some(launch) = capability_launch.as_ref() {
            environment.extend(launch.environment.iter().cloned());
        }
        let report = capability_plan.map_or_else(Default::default, |plan| {
            plan.enforcement_report(crate::access::EnforcementEvidence::advisory_only())
        });
        Ok(LaunchSpec::new(
            Self::command_for_with_policy(
                &self.command,
                request.session_plan(),
                request.initial_prompt(),
                request.access_policy().boundary_prompt(),
                overrides,
            ),
            request.workspace().root().to_path_buf(),
            environment,
            HookMetadata::none(),
        )
        .with_capabilities(report))
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
        false
    }

    fn response_id(&self, session: &AgentSession) -> String {
        let identity = format!("brain://codex/response/{}", session.as_str());
        uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, identity.as_bytes()).to_string()
    }

    fn can_resume_response_session(&self) -> bool {
        false
    }
}
