//! Claude Code translation behind the frontend-neutral agent facade.

use std::path::PathBuf;

use crate::agent::{
    AgentError, AgentFrontend, AgentKind, AgentSession, CompletionStrategy, HookMetadata,
    InputSequence, LaunchRequest, LaunchSpec, SessionPlan,
    frontend::{launch_environment, shell_quote},
};

mod session_registry;
mod transcript;

#[cfg(test)]
pub(crate) use session_registry::SessionClaim;
pub(crate) use session_registry::session_is_held_by_live_process;
pub(crate) use transcript::transcript_has_conversation;
use session_registry::read_session_claims;

pub(crate) const DEFAULT_COMMAND: &str = "claude --dangerously-skip-permissions";

/// Claude Code command, input, completion, and transcript conventions.
pub(crate) struct ClaudeFrontend {
    command: String,
    workspace_root: PathBuf,
    projects_dir: Option<PathBuf>,
    sessions_dir: Option<PathBuf>,
    pid_alive: crate::state::PidAlive,
}

impl ClaudeFrontend {
    /// Construct a Claude adapter from its effective command and transcript roots.
    #[must_use]
    pub fn new(command: impl Into<String>, workspace_root: PathBuf, projects_dir: PathBuf) -> Self {
        let command = command.into();
        let command = command.trim();
        // Claude keeps its per-process session registry next to the transcript
        // projects tree, both under `~/.claude`.
        let sessions_dir = projects_dir
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(|parent| parent.join("sessions"));
        Self {
            command: if command.is_empty() {
                DEFAULT_COMMAND.to_owned()
            } else {
                command.to_owned()
            },
            workspace_root,
            projects_dir: Some(projects_dir),
            sessions_dir,
            pid_alive: crate::state::system_pid_alive,
        }
    }

    /// Swap the process-liveness probe so resume eligibility can be exercised
    /// without a real Claude process.
    #[cfg(test)]
    #[must_use]
    pub(super) fn with_pid_probe(mut self, pid_alive: crate::state::PidAlive) -> Self {
        self.pid_alive = pid_alive;
        self
    }

    pub(super) fn without_projects_dir(
        command: impl Into<String>,
        workspace_root: PathBuf,
    ) -> Self {
        let mut frontend = Self::new(command, workspace_root, PathBuf::new());
        frontend.projects_dir = None;
        frontend.sessions_dir = None;
        frontend
    }

    pub(super) fn command_for(command: &str, plan: &SessionPlan, prompt: Option<&str>) -> String {
        Self::command_for_with_policy(command, plan, prompt, None, None)
    }

    fn command_for_with_policy(
        command: &str,
        plan: &SessionPlan,
        prompt: Option<&str>,
        policy: Option<&str>,
        mcp_config: Option<&std::path::Path>,
    ) -> String {
        let mut parts = vec![command.trim().to_owned()];
        if let Some(path) = mcp_config {
            parts.push("--mcp-config".to_owned());
            parts.push(shell_quote(&path.display().to_string()));
            parts.push("--strict-mcp-config".to_owned());
        }
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

    pub(crate) fn mcp_enforcement_evidence(command: &str) -> crate::access::EnforcementEvidence {
        if is_direct_claude_invocation(command) {
            crate::access::EnforcementEvidence::strict_mcps_only()
        } else {
            crate::access::EnforcementEvidence::advisory_only()
        }
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

    /// A transcript Claude can actually resume: present *and* holding at least
    /// one real turn.
    fn resumable_transcript(&self, session: &AgentSession) -> Option<PathBuf> {
        let path = self.existing_transcript(session)?;
        let contents = std::fs::read_to_string(&path).ok()?;
        transcript_has_conversation(&contents).then_some(path)
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

    /// Whether another live process still owns the session. Claude refuses
    /// `--resume` for one of those, so brain must not offer it as a candidate
    /// however complete its transcript is.
    fn session_is_held_elsewhere(&self, session: &AgentSession) -> bool {
        self.sessions_dir.as_ref().is_some_and(|directory| {
            session_is_held_by_live_process(
                &read_session_claims(directory),
                session.as_str(),
                self.pid_alive,
            )
        })
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
        let capability_plan = request.access_policy().capability_plan();
        if request.access_policy().mode() == crate::access::AccessMode::Unrestricted {
            crate::access::cleanup_workspace_capabilities(request.workspace())
                .map_err(|error| AgentError::Frontend(error.to_string()))?;
        } else if capability_plan.is_some() {
            crate::access::prepare_workspace_capabilities(request.workspace())
                .and_then(|()| crate::access::cleanup_codex_runtime_artifacts(request.workspace()))
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
        let mcp_config = capability_plan
            .filter(|plan| !plan.mcps.uses_global_configuration())
            .map(|plan| {
                let path = request.workspace().paths().capability_mcp_config();
                crate::access::write_claude_runtime_config(request.workspace(), plan)
                    .map_err(|error| AgentError::Frontend(error.to_string()))?;
                Ok::<_, AgentError>(path)
            })
            .transpose()?;
        let report = capability_plan.map_or_else(Default::default, |plan| {
            let evidence = if mcp_config.is_some() {
                Self::mcp_enforcement_evidence(&self.command)
            } else {
                crate::access::EnforcementEvidence::advisory_only()
            };
            plan.enforcement_report(evidence)
        });
        Ok(LaunchSpec::new(
            Self::command_for_with_policy(
                &self.command,
                request.session_plan(),
                request.initial_prompt(),
                request.access_policy().boundary_prompt(),
                mcp_config.as_deref(),
            ),
            request.workspace().root().to_path_buf(),
            launch_environment(request, self.kind()),
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
                InputSequence::text_then_key(text, b"\r")
            }
            crate::agent::AgentAction::StartNewSession => InputSequence::bytes(b"/new\r"),
        })
    }

    fn completion_strategy(&self) -> Result<CompletionStrategy, AgentError> {
        Ok(CompletionStrategy::Hook)
    }

    fn resume_candidate_exists(&self, session: &AgentSession) -> Result<bool, AgentError> {
        Ok(self.resumable_transcript(session).is_some() && !self.session_is_held_elsewhere(session))
    }

    fn response_id(&self, session: &AgentSession) -> Result<String, AgentError> {
        Ok(session.as_str().to_owned())
    }

    fn can_resume_response_session(&self, _session: &AgentSession) -> Result<bool, AgentError> {
        Ok(true)
    }
}

fn is_direct_claude_invocation(command: &str) -> bool {
    let Some(arguments) = parse_direct_command(command) else {
        return false;
    };
    let Some(executable) = arguments.first() else {
        return false;
    };
    if std::path::Path::new(executable)
        .file_name()
        .and_then(|name| name.to_str())
        != Some("claude")
    {
        return false;
    }
    !arguments.iter().skip(1).any(|argument| {
        const OWNED_FLAGS: [&str; 7] = [
            "--",
            "--mcp-config",
            "--strict-mcp-config",
            "--append-system-prompt",
            "--session-id",
            "--resume",
            "--bare",
        ];
        OWNED_FLAGS.iter().any(|flag| {
            argument == flag || (*flag != "--" && argument.starts_with(&format!("{flag}=")))
        })
    })
}

fn parse_direct_command(command: &str) -> Option<Vec<String>> {
    #[derive(Clone, Copy)]
    enum Quote {
        None,
        Single,
        Double,
    }

    let mut arguments = Vec::new();
    let mut argument = String::new();
    let mut quote = Quote::None;
    let mut escaped = false;
    let mut started = false;
    for character in command.trim().chars() {
        if character.is_control() && character != '\t' {
            return None;
        }
        if escaped {
            argument.push(character);
            escaped = false;
            started = true;
            continue;
        }
        match quote {
            Quote::Single => {
                if character == '\'' {
                    quote = Quote::None;
                } else {
                    argument.push(character);
                }
                started = true;
            }
            Quote::Double => match character {
                '"' => quote = Quote::None,
                '\\' => escaped = true,
                '$' | '`' => return None,
                _ => {
                    argument.push(character);
                    started = true;
                }
            },
            Quote::None => match character {
                '\'' => {
                    quote = Quote::Single;
                    started = true;
                }
                '"' => {
                    quote = Quote::Double;
                    started = true;
                }
                '\\' => {
                    escaped = true;
                    started = true;
                }
                ' ' | '\t' => {
                    if started {
                        arguments.push(std::mem::take(&mut argument));
                        started = false;
                    }
                }
                ';' | '|' | '&' | '<' | '>' | '(' | ')' | '#' | '$' | '`' | '*' | '?' | '['
                | ']' | '{' | '}' => return None,
                _ => {
                    argument.push(character);
                    started = true;
                }
            },
        }
    }
    if escaped || !matches!(quote, Quote::None) {
        return None;
    }
    if started {
        arguments.push(argument);
    }
    Some(arguments)
}

fn append_prompt(parts: &mut Vec<String>, prompt: Option<&str>) {
    if let Some(prompt) = prompt {
        let prompt = prompt.trim();
        if !prompt.is_empty() {
            parts.push("--".to_owned());
            parts.push(shell_quote(prompt));
        }
    }
}
