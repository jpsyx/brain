//! pi translation behind the frontend-neutral agent facade.

mod probe;
pub(super) mod sessions;

use std::path::{Path, PathBuf};

use crate::agent::{
    AgentError, AgentFrontend, AgentKind, AgentSession, CompletionStrategy, HookMetadata,
    InputSequence, LaunchRequest, LaunchSpec, SessionPlan,
    frontend::{launch_environment, shell_quote},
};

use sessions::ProcessSessionDirOverrides;

pub(crate) const DEFAULT_COMMAND: &str = "pi";

/// Where Brain installs pi's lifecycle bridge inside a workspace.
///
/// It lives beside the frontend-neutral Python bridges rather than in
/// `.pi/extensions/`, because pi loads a `-e` extension before it resolves
/// project trust and would otherwise discover the same file twice.
pub(crate) const EXTENSION_RELATIVE_PATH: &str = ".brain/hooks/pi_brain_extension.ts";

/// Alt+Enter as a kitty-protocol CSI u sequence (codepoint 13, modifier
/// `1 + alt`), which pi decodes as `alt+enter` with or without that protocol
/// active.
const ALT_ENTER: &[u8] = b"\x1b[13;3u";

/// Brain's rendered skills inside a workspace, which pi is pointed at directly.
const WORKSPACE_SKILLS_RELATIVE_PATH: &str = ".agents/skills";

/// Flags Brain appends to a pi launch. A configured command that already
/// carries one of them is a wrapper whose selection Brain cannot vouch for.
const PI_OWNED_FLAGS: [&str; 7] = [
    "--",
    "--session-id",
    "--append-system-prompt",
    "--skill",
    "--no-skills",
    "--no-approve",
    "--extension",
];

pub(crate) fn compatibility_version(command: &str) -> Result<Option<String>, AgentError> {
    probe::compatibility(command)
}

/// Which skill sources one launch exposes.
enum SkillSelection<'a> {
    /// Only these directories: pi's own discovery is switched off.
    Exactly(&'a Path),
    /// Brain's rendered workspace skills alongside pi's ordinary discovery.
    WorkspaceAndDiscovered(PathBuf),
    /// Nothing for Brain to add.
    Discovered,
}

/// pi command, input, completion, and session conventions.
pub(crate) struct PiFrontend {
    command: String,
    workspace_root: PathBuf,
    home: Option<PathBuf>,
    session_dir_overrides: ProcessSessionDirOverrides,
}

impl PiFrontend {
    /// Construct a pi adapter scoped to one resolved workspace root.
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
            home: std::env::var_os("HOME").map(PathBuf::from),
            session_dir_overrides: ProcessSessionDirOverrides::from_environment(),
        }
    }

    /// Construct a pi adapter for the current directory, for tests.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn new(command: impl Into<String>) -> Self {
        Self::for_workspace(
            command,
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        )
    }

    /// Point session discovery at a specific tree, for tests.
    #[cfg(test)]
    #[must_use]
    pub(super) fn with_session_dir(mut self, session_dir: Option<&Path>) -> Self {
        self.session_dir_overrides =
            ProcessSessionDirOverrides::for_session_dir(session_dir.map(Path::to_path_buf));
        self
    }

    pub(super) fn command_for(command: &str, plan: &SessionPlan, prompt: Option<&str>) -> String {
        Self::command_for_with_capabilities(
            command,
            plan,
            prompt,
            None,
            &SkillSelection::Discovered,
            None,
        )
    }

    /// pi opens the named session and creates it when missing, so one flag
    /// serves both plans and Brain's chosen id stays authoritative either way.
    fn command_for_with_capabilities(
        command: &str,
        plan: &SessionPlan,
        prompt: Option<&str>,
        policy: Option<&str>,
        skills: &SkillSelection<'_>,
        extension: Option<&Path>,
    ) -> String {
        // Brain's own workspace resources are passed explicitly below, so
        // declining project-local trust costs nothing and keeps an unattended
        // panel from ever stopping on pi's trust prompt.
        let mut parts = vec![command.trim().to_owned(), "--no-approve".to_owned()];
        match skills {
            SkillSelection::Exactly(directory) => {
                parts.push("--no-skills".to_owned());
                parts.push("--skill".to_owned());
                parts.push(shell_quote(&directory.display().to_string()));
            }
            SkillSelection::WorkspaceAndDiscovered(directory) => {
                parts.push("--skill".to_owned());
                parts.push(shell_quote(&directory.display().to_string()));
            }
            SkillSelection::Discovered => {}
        }
        if let Some(policy) = policy {
            parts.push("--append-system-prompt".to_owned());
            parts.push(shell_quote(policy));
        }
        if let Some(extension) = extension {
            parts.push("--extension".to_owned());
            parts.push(shell_quote(&extension.display().to_string()));
        }
        parts.push("--session-id".to_owned());
        parts.push(shell_quote(plan.session().as_str()));
        if let Some(prompt) = prompt.map(str::trim).filter(|prompt| !prompt.is_empty()) {
            parts.push("--".to_owned());
            parts.push(shell_quote(prompt));
        }
        parts.join(" ")
    }

    /// pi has no MCP mechanism at all, and its skill selection is exact only
    /// when Brain is the one appending `--no-skills --skill` to a plain `pi`.
    pub(crate) fn capability_evidence(command: &str) -> crate::access::EnforcementEvidence {
        if is_direct_pi_invocation(command) {
            crate::access::EnforcementEvidence::strict_skills_without_mcp_support()
        } else {
            crate::access::EnforcementEvidence::without_mcp_support()
        }
    }

    fn session_directory(&self) -> Option<PathBuf> {
        sessions::session_directory(
            &self.workspace_root,
            self.home.as_deref(),
            self.session_dir_overrides.as_overrides(),
        )
    }

    fn session_recorded(&self, session: &AgentSession) -> bool {
        self.session_directory()
            .is_some_and(|directory| sessions::session_exists(&directory, session.as_str()))
    }

    fn workspace_skills_dir(&self) -> Option<PathBuf> {
        let directory = self.workspace_root.join(WORKSPACE_SKILLS_RELATIVE_PATH);
        directory.is_dir().then_some(directory)
    }
}

fn is_direct_pi_invocation(command: &str) -> bool {
    crate::agent::direct_command::is_direct_invocation(command, "pi", &PI_OWNED_FLAGS)
}

impl AgentFrontend for PiFrontend {
    fn kind(&self) -> AgentKind {
        AgentKind::Pi
    }

    fn ensure_available(&self) -> Result<(), AgentError> {
        probe::ensure_compatible(&self.command)
    }

    fn launch_spec(&self, request: &LaunchRequest) -> Result<LaunchSpec, AgentError> {
        let capability_plan = request.access_policy().capability_plan();
        if request.access_policy().mode() == crate::access::AccessMode::Unrestricted {
            crate::access::cleanup_workspace_capabilities(request.workspace())
                .map_err(|error| AgentError::Frontend(error.to_string()))?;
        } else if capability_plan.is_some() {
            crate::access::prepare_workspace_capabilities(request.workspace())
                .and_then(|()| crate::access::cleanup_claude_runtime_artifacts(request.workspace()))
                .and_then(|()| crate::access::cleanup_codex_runtime_artifacts(request.workspace()))
                .map_err(|error| AgentError::Frontend(error.to_string()))?;
        }
        let selected_skills =
            capability_plan.filter(|plan| !plan.skills.uses_global_configuration());
        if let Some(plan) = selected_skills {
            crate::skills::render_workspace_capabilities(
                request.workspace(),
                request.actor(),
                plan,
            )
            .map_err(|error| AgentError::Frontend(error.to_string()))?;
        }
        let capability_skills = selected_skills.map(|_| {
            request
                .workspace()
                .paths()
                .capability_skills_dir(request.actor().user_id())
        });
        let skills = capability_skills.as_deref().map_or_else(
            || {
                self.workspace_skills_dir()
                    .map_or(SkillSelection::Discovered, |directory| {
                        SkillSelection::WorkspaceAndDiscovered(directory)
                    })
            },
            SkillSelection::Exactly,
        );
        let evidence = if capability_skills.is_some() {
            Self::capability_evidence(&self.command)
        } else {
            crate::access::EnforcementEvidence::without_mcp_support()
        };
        let report =
            capability_plan.map_or_else(Default::default, |plan| plan.enforcement_report(evidence));
        Ok(LaunchSpec::new(
            Self::command_for_with_capabilities(
                &self.command,
                request.session_plan(),
                request.initial_prompt(),
                request.access_policy().boundary_prompt(),
                &skills,
                Some(&self.workspace_root.join(EXTENSION_RELATIVE_PATH)),
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
            // Alt+Enter, pi's follow-up queue: delivered once the agent has
            // finished all of its work, and treated as an ordinary submit when
            // pi is idle. Enter would instead *steer* a running turn, folding
            // Brain's separate request into whatever pi is already doing.
            //
            // Sent as the CSI u encoding rather than `ESC CR`, because pi reads
            // `ESC CR` as shift+enter (a newline, not a submit) whenever its
            // kitty keyboard protocol is active.
            crate::agent::AgentAction::FollowUpAfterActiveTurn(text) => {
                InputSequence::text_then_key(text, ALT_ENTER)
            }
            crate::agent::AgentAction::StartNewSession => InputSequence::bytes(b"/new\r"),
        })
    }

    fn completion_strategy(&self) -> Result<CompletionStrategy, AgentError> {
        Ok(CompletionStrategy::Hook)
    }

    fn resume_candidate_exists(&self, session: &AgentSession) -> Result<bool, AgentError> {
        Ok(self.session_recorded(session))
    }

    fn response_id(&self, session: &AgentSession) -> Result<String, AgentError> {
        let identity = format!("brain://pi/response/{}", session.as_str());
        Ok(uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, identity.as_bytes()).to_string())
    }

    // An SMS or email follow-up resumes on the same evidence an interactive one
    // does, so the two channels cannot disagree about what is resumable.
    fn can_resume_response_session(&self, session: &AgentSession) -> Result<bool, AgentError> {
        Ok(self.session_recorded(session))
    }
}

#[cfg(test)]
mod frontend_tests;
