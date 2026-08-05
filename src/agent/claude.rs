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

    fn transcript(&self, session: &AgentSession) -> Result<Option<PathBuf>, AgentError> {
        Ok(self
            .existing_transcript(session)
            .or_else(|| self.transcript_path(session)))
    }

    fn resume_candidate_exists(&self, session: &AgentSession) -> Result<bool, AgentError> {
        Ok(self.existing_transcript(session).is_some())
    }

    fn response_id(&self, session: &AgentSession) -> Result<String, AgentError> {
        Ok(session.as_str().to_owned())
    }

    fn can_resume_response_session(&self) -> Result<bool, AgentError> {
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
