//! OpenAI Codex translation behind the frontend-neutral agent facade.

use std::path::PathBuf;

use crate::agent::{
    AgentError, AgentFrontend, AgentKind, AgentSession, CompletionStrategy, HookMetadata,
    InputSequence, LaunchRequest, LaunchSpec, SessionPlan,
    frontend::{launch_environment, shell_quote},
};

mod sessions;

pub(crate) const DEFAULT_COMMAND: &str = "codex";

/// Codex command, input, completion, and transcript conventions.
pub(crate) struct CodexFrontend {
    command: String,
    /// Where Codex records its rollouts, or `None` when this machine has no home
    /// directory to resolve — in which case no session is treated as resumable.
    sessions_dir: Option<PathBuf>,
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
            sessions_dir: default_sessions_dir(),
        }
    }

    /// Point the resume check at a specific rollout tree, for tests.
    #[cfg(test)]
    pub(super) fn with_sessions_dir(mut self, sessions_dir: Option<PathBuf>) -> Self {
        self.sessions_dir = sessions_dir;
        self
    }

    /// Whether Codex still holds a rollout for this session.
    fn rollout_exists(&self, session: &AgentSession) -> bool {
        self.sessions_dir
            .as_deref()
            .and_then(|root| sessions::find_rollout(root, session.as_str()))
            .is_some()
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
                InputSequence::text_then_key(text, b"\t")
            }
            crate::agent::AgentAction::StartNewSession => InputSequence::bytes(b"/new\t"),
        })
    }

    fn completion_strategy(&self) -> Result<CompletionStrategy, AgentError> {
        Ok(CompletionStrategy::Hook)
    }

    fn resume_candidate_exists(&self, session: &AgentSession) -> Result<bool, AgentError> {
        Ok(self.rollout_exists(session))
    }

    fn response_id(&self, session: &AgentSession) -> Result<String, AgentError> {
        let identity = format!("brain://codex/response/{}", session.as_str());
        Ok(uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, identity.as_bytes()).to_string())
    }

    // An SMS or email follow-up resumes exactly as an interactive one does: the
    // rollout is the same evidence either way, so the two channels cannot drift.
    fn can_resume_response_session(&self, session: &AgentSession) -> Result<bool, AgentError> {
        Ok(self.rollout_exists(session))
    }
}

/// `~/.codex/sessions`, Codex's own rollout location.
fn default_sessions_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".codex").join("sessions"))
}

#[cfg(test)]
mod frontend_tests {
    use super::{CodexFrontend, sessions};
    use crate::agent::{AgentFrontend, AgentSession, SessionPlan};

    const ID: &str = "019feb9e-edc0-7252-945a-5e06a30e0eec";

    fn session() -> AgentSession {
        AgentSession::new(ID).expect("a nonempty session id")
    }

    fn tree_with_rollout() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        let day = root.path().join("2026/08/11");
        std::fs::create_dir_all(&day).unwrap();
        std::fs::write(
            day.join(format!("rollout-2026-08-11T09-49-49-{ID}.jsonl")),
            b"{}\n",
        )
        .unwrap();
        root
    }

    /// Both channels read the same evidence, so SMS and email can never disagree
    /// with the interactive panel about whether a session can be picked back up.
    #[test]
    fn a_recorded_session_is_resumable_for_every_channel() {
        let root = tree_with_rollout();
        let frontend =
            CodexFrontend::new("codex").with_sessions_dir(Some(root.path().to_path_buf()));

        assert!(frontend.resume_candidate_exists(&session()).unwrap());
        assert!(frontend.can_resume_response_session(&session()).unwrap());
    }

    #[test]
    fn a_session_codex_no_longer_holds_is_resumable_for_no_channel() {
        let root = tempfile::tempdir().unwrap();
        let frontend =
            CodexFrontend::new("codex").with_sessions_dir(Some(root.path().to_path_buf()));

        assert!(!frontend.resume_candidate_exists(&session()).unwrap());
        assert!(!frontend.can_resume_response_session(&session()).unwrap());
    }

    /// Without a resolvable home there is no evidence either way, and guessing
    /// would resume into a session that may not exist.
    #[test]
    fn no_sessions_directory_means_nothing_is_resumable() {
        let frontend = CodexFrontend::new("codex").with_sessions_dir(None);

        assert!(!frontend.resume_candidate_exists(&session()).unwrap());
        assert!(!frontend.can_resume_response_session(&session()).unwrap());
    }

    /// The command brain builds for a validated session is Codex's own resume
    /// verb, so the id we validated is the id Codex reopens.
    #[test]
    fn resuming_builds_codex_resume_with_the_validated_id() {
        let plan = SessionPlan::Resume(session());
        let command = CodexFrontend::command_for("codex", &plan, None);

        // The id is shell-quoted, as every interpolated value is.
        assert_eq!(command, format!("codex resume '{ID}'"));
        assert!(sessions::rollout_matches(
            &format!("rollout-2026-08-11T09-49-49-{ID}.jsonl"),
            ID
        ));
    }
}
