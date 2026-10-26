//! OpenCode translation behind the frontend-neutral agent facade.

mod config;
mod instructions;
mod probe;
mod session;

#[cfg(test)]
pub(crate) use probe::seed_fixture_compatibility;

#[cfg(test)]
mod instructions_tests;

use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

use crate::agent::{
    AgentError, AgentFrontend, AgentKind, AgentSession, CompletionStrategy, HookMetadata,
    InputSequence, LaunchRequest, LaunchSpec, SessionPlan,
    frontend::{launch_environment, shell_quote},
};

pub(crate) const DEFAULT_COMMAND: &str = "opencode";

pub(crate) fn compatibility_version(command: &str) -> Result<Option<String>, AgentError> {
    probe::compatibility(command).map(|report| report.version().map(str::to_owned))
}

/// OpenCode command, input, completion, and session conventions.
pub(crate) struct OpenCodeFrontend {
    command: String,
    workspace_root: PathBuf,
    inherited_config: Option<String>,
    session_snapshot: Mutex<Option<Result<session::SessionSnapshot, AgentError>>>,
}

impl OpenCodeFrontend {
    /// Construct an OpenCode adapter from its effective launch command.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn new(command: impl Into<String>) -> Self {
        Self::for_workspace(
            command,
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        )
    }

    /// Construct an OpenCode adapter scoped to one resolved workspace root.
    #[must_use]
    pub(crate) fn for_workspace(
        command: impl Into<String>,
        workspace_root: impl Into<PathBuf>,
    ) -> Self {
        let command = command.into();
        let command = command.trim();
        Self {
            command: if command.is_empty() {
                DEFAULT_COMMAND.to_owned()
            } else {
                command.to_owned()
            },
            workspace_root: workspace_root.into(),
            inherited_config: std::env::var("OPENCODE_CONFIG_CONTENT").ok(),
            session_snapshot: Mutex::new(None),
        }
    }

    fn discover_once(&self) -> Result<session::SessionSnapshot, AgentError> {
        let mut snapshot = self.session_snapshot.lock().map_err(|_| {
            AgentError::Frontend("OpenCode session discovery cache is unavailable".to_owned())
        })?;
        snapshot
            .get_or_insert_with(|| session::discover(&self.command, &self.workspace_root))
            .clone()
    }

    fn clear_discovery(&self) {
        if let Ok(mut snapshot) = self.session_snapshot.lock() {
            *snapshot = None;
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

    fn ensure_available(&self) -> Result<(), AgentError> {
        instructions::validate_prefix(&self.command)?;
        probe::ensure_compatible(&self.command)
    }

    fn launch_spec(&self, request: &LaunchRequest) -> Result<LaunchSpec, AgentError> {
        self.clear_discovery();
        let mut inherited = config::parse(self.inherited_config.as_deref())?;
        let mut environment = launch_environment(request, self.kind());
        environment.retain(|(name, _)| name != "OPENCODE_CONFIG_CONTENT");
        if let Some(config) = &self.inherited_config {
            environment.push(("OPENCODE_CONFIG_CONTENT".to_owned(), config.clone()));
        }
        if request.instructions().is_some() {
            let prior =
                instructions::resolve(&self.command, request.workspace().root(), &environment)?;
            config::set_effective_prompt(&mut inherited, prior)?;
        }
        let instruction_text = request.instruction_text();
        let brain_prompt = instruction_text.as_deref().unwrap_or(
            "You are the Brain agent. Follow the user's request and report completion clearly.",
        );
        let capability_plan = request.access_policy().capability_plan();
        let mcp_launch = capability_plan
            .filter(|plan| !plan.mcps.uses_global_configuration())
            .map(crate::access::opencode_mcp_launch);
        let selected_skill_names = capability_plan
            .filter(|plan| !plan.skills.uses_global_configuration())
            .map(|plan| plan.skills.available_names());
        let selected_directory = capability_plan
            .filter(|plan| !plan.skills.uses_global_configuration())
            .map(|plan| {
                request
                    .render_capabilities(plan)
                    .map(|report| report.rendered_dir)
            })
            .transpose()?;
        let selected_skills = selected_directory
            .as_ref()
            .zip(selected_skill_names.as_ref());
        let config = config::merge(
            inherited,
            brain_prompt,
            mcp_launch.as_ref().map(|launch| launch.entries.clone()),
            selected_skills
                .as_ref()
                .map(|(path, names)| (path.as_path(), names.as_slice())),
        )?;
        if request.access_policy().mode() == crate::access::AccessMode::Unrestricted {
            crate::access::cleanup_workspace_capabilities(request.workspace())
                .map_err(|error| AgentError::Frontend(error.to_string()))?;
        } else if capability_plan.is_some() {
            crate::access::prepare_workspace_capabilities(request.workspace())
                .and_then(|()| crate::access::cleanup_claude_runtime_artifacts(request.workspace()))
                .and_then(|()| crate::access::cleanup_codex_runtime_artifacts(request.workspace()))
                .map_err(|error| AgentError::Frontend(error.to_string()))?;
        }
        if let Some(plan) = capability_plan.filter(|plan| !plan.skills.uses_global_configuration())
        {
            request.render_capabilities(plan)?;
        }
        environment.retain(|(name, _)| name != "OPENCODE_CONFIG_CONTENT");
        if let Some(launch) = mcp_launch.as_ref() {
            environment.extend(launch.environment.iter().cloned());
        }
        environment.push(("OPENCODE_CONFIG_CONTENT".to_owned(), config));
        let capabilities = capability_plan.map_or_else(Default::default, |plan| {
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

    fn rollback_launch(&self, request: &LaunchRequest) -> Result<(), AgentError> {
        crate::access::cleanup_workspace_capabilities(request.workspace())
            .map_err(|error| AgentError::Frontend(error.to_string()))
    }

    fn input_for(
        &self,
        action: crate::agent::AgentAction<'_>,
    ) -> Result<InputSequence, AgentError> {
        Ok(match action {
            crate::agent::AgentAction::TypeText(text) => InputSequence::text(text),
            crate::agent::AgentAction::SubmitNow => InputSequence::bytes(b"\r"),
            crate::agent::AgentAction::FollowUpAfterActiveTurn(text) => {
                InputSequence::text_then_key(text, b"\r")
            }
            crate::agent::AgentAction::StartNewSession => InputSequence::bytes(b"/new\r"),
        })
    }

    fn completion_strategy(&self) -> Result<CompletionStrategy, AgentError> {
        Ok(CompletionStrategy::Hook)
    }

    fn resume_candidate_exists(&self, session: &AgentSession) -> Result<bool, AgentError> {
        self.discover_once()
            .map(|snapshot| snapshot.contains(session))
    }

    fn response_id(&self, session: &AgentSession) -> Result<String, AgentError> {
        let identity = format!("brain://opencode/response/{}", session.as_str());
        Ok(uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, identity.as_bytes()).to_string())
    }

    fn can_resume_response_session(&self, session: &AgentSession) -> Result<bool, AgentError> {
        session::discover(&self.command, Path::new(&self.workspace_root))
            .map(|snapshot| snapshot.contains(session))
    }
}
