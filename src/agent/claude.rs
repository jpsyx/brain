//! Claude Code translation behind the frontend-neutral agent facade.

use std::path::PathBuf;

use crate::agent::{
    AgentError, AgentFrontend, AgentKind, AgentSession, CompletionStrategy, HookMetadata,
    InputSequence, LaunchRequest, LaunchSpec, SessionPlan,
    frontend::{launch_environment, shell_quote},
};

pub(crate) const DEFAULT_COMMAND: &str = "claude --dangerously-skip-permissions";

/// Claude Code command, input, completion, and transcript conventions.
pub struct ClaudeFrontend {
    command: String,
    workspace_root: PathBuf,
    projects_dir: Option<PathBuf>,
}

impl ClaudeFrontend {
    /// Construct a Claude adapter from its effective command and transcript roots.
    #[must_use]
    pub fn new(command: impl Into<String>, workspace_root: PathBuf, projects_dir: PathBuf) -> Self {
        let command = command.into();
        let command = command.trim();
        Self {
            command: if command.is_empty() {
                DEFAULT_COMMAND.to_owned()
            } else {
                command.to_owned()
            },
            workspace_root,
            projects_dir: Some(projects_dir),
        }
    }

    pub(super) fn without_projects_dir(
        command: impl Into<String>,
        workspace_root: PathBuf,
    ) -> Self {
        let mut frontend = Self::new(command, workspace_root, PathBuf::new());
        frontend.projects_dir = None;
        frontend
    }

    pub(super) fn command_for(command: &str, plan: &SessionPlan, prompt: Option<&str>) -> String {
        Self::command_for_with_policy(command, plan, prompt, None)
    }

    fn command_for_with_policy(
        command: &str,
        plan: &SessionPlan,
        prompt: Option<&str>,
        policy: Option<&str>,
    ) -> String {
        let mut parts = vec![command.trim().to_owned()];
        if let Some(policy) = policy {
            parts.push("--append-system-prompt".to_owned());
            parts.push(shell_quote(policy));
        }
        match plan {
            SessionPlan::Fresh(session) => {
                parts.push("--session-id".to_owned());
                parts.push(shell_quote(session.as_str()));
            }
            SessionPlan::Resume(session) => {
                parts.push("--resume".to_owned());
                parts.push(shell_quote(session.as_str()));
            }
        }
        append_prompt(&mut parts, prompt);
        parts.join(" ")
    }

    fn transcript_path(&self, session: &AgentSession) -> Option<PathBuf> {
        let project = project_dir_name(&self.workspace_root);
        Some(
            self.projects_dir
                .as_ref()?
                .join(project)
                .join(format!("{}.jsonl", session.as_str())),
        )
    }

    fn existing_transcript(&self, session: &AgentSession) -> Option<PathBuf> {
        let primary = self.transcript_path(session)?;
        if primary.is_file() {
            return Some(primary);
        }
        let file = format!("{}.jsonl", session.as_str());
        std::fs::read_dir(self.projects_dir.as_ref()?)
            .ok()?
            .flatten()
            .map(|entry| entry.path().join(&file))
            .find(|candidate| candidate.is_file())
    }
}

/// Claude's project-directory name for a workspace root.
#[must_use]
pub(crate) fn project_dir_name(workspace_root: &std::path::Path) -> String {
    workspace_root.to_string_lossy().replace(['/', '.'], "-")
}

impl AgentFrontend for ClaudeFrontend {
    fn kind(&self) -> AgentKind {
        AgentKind::Claude
    }

    fn launch_spec(&self, request: &LaunchRequest) -> Result<LaunchSpec, AgentError> {
        Ok(LaunchSpec::new(
            Self::command_for_with_policy(
                &self.command,
                request.session_plan(),
                request.initial_prompt(),
                request.access_policy().boundary_prompt(),
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
        InputSequence::bytes(b"\r")
    }

    fn new_session_input(&self) -> InputSequence {
        InputSequence::bytes(b"/new\r")
    }

    fn completion_strategy(&self) -> CompletionStrategy {
        CompletionStrategy::Hook
    }

    fn transcript(&self, session: &AgentSession) -> Option<PathBuf> {
        self.existing_transcript(session)
            .or_else(|| self.transcript_path(session))
    }

    fn resume_candidate_exists(&self, session: &AgentSession) -> bool {
        self.existing_transcript(session).is_some()
    }

    fn response_id(&self, session: &AgentSession) -> String {
        session.as_str().to_owned()
    }

    fn can_resume_response_session(&self) -> bool {
        true
    }
}

fn append_prompt(parts: &mut Vec<String>, prompt: Option<&str>) {
    if let Some(prompt) = prompt {
        let prompt = prompt.trim();
        if !prompt.is_empty() {
            parts.push(shell_quote(prompt));
        }
    }
}
