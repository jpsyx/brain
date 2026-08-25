//! The frontend-facing half of the agent facade boundary.

use std::{path::PathBuf, sync::Arc};

use crate::{
    access::{AccessMode, AccessPolicy},
    actor::{ActorContext, Channel},
    agent::{
        AgentError, AgentKind, AgentSession, CompletionStrategy, HookMetadata, InputSequence,
        SessionPlan,
    },
    workspace::WorkspaceContext,
};

pub(crate) const SHELL_COMMAND_ARGUMENT_BUDGET_BYTES: usize = 96 * 1024;
pub(crate) const SHELL_COMMAND_FIXED_OVERHEAD_BUDGET_BYTES: usize = 12 * 1024;
const SHELL_QUOTE_DELIMITER_BYTES: usize = 2;
const SHELL_QUOTED_EXPANSION_NUMERATOR: usize = 7;
const SHELL_QUOTED_EXPANSION_DENOMINATOR: usize = 4;
const SHELL_INLINE_VALUE_UNROUNDED_BYTES: usize = (SHELL_COMMAND_ARGUMENT_BUDGET_BYTES
    - SHELL_COMMAND_FIXED_OVERHEAD_BUDGET_BYTES
    - SHELL_QUOTE_DELIMITER_BYTES)
    * SHELL_QUOTED_EXPANSION_DENOMINATOR
    / SHELL_QUOTED_EXPANSION_NUMERATOR;
pub(crate) const SHELL_INLINE_VALUE_BUDGET_BYTES: usize =
    SHELL_INLINE_VALUE_UNROUNDED_BYTES / 1024 * 1024;

/// Frontend-neutral input intent translated atomically by one adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentAction<'a> {
    /// Type literal text without submitting it.
    TypeText(&'a str),
    /// Submit the text already present in the frontend composer.
    SubmitNow,
    /// Send a follow-up using the frontend's native busy-turn behavior.
    FollowUpAfterActiveTurn(&'a str),
    /// Begin a fresh conversation through the frontend's own command surface.
    StartNewSession,
}

/// All frontend-neutral inputs required to launch an agent.
#[derive(Clone)]
pub struct LaunchRequest {
    workspace: Arc<WorkspaceContext>,
    actor: ActorContext,
    session_plan: SessionPlan,
    initial_prompt: Option<String>,
    access_policy: AccessPolicy,
    channel: Channel,
    hook_metadata: HookMetadata,
}

impl std::fmt::Debug for LaunchRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LaunchRequest")
            .field("workspace_id", &self.workspace.id())
            .field("actor_id", &self.actor.user_id())
            .field("session_plan", &self.session_plan)
            .field(
                "initial_prompt",
                &self.initial_prompt.as_ref().map(|_| "<redacted>"),
            )
            .field("access_policy", &self.access_policy)
            .field("channel", &self.channel)
            .field("hook_metadata", &self.hook_metadata)
            .finish()
    }
}

impl LaunchRequest {
    /// Build a launch request from resolved trusted workspace state.
    #[must_use]
    pub fn from_trusted_context(
        workspace: Arc<WorkspaceContext>,
        actor: ActorContext,
        session_plan: SessionPlan,
        initial_prompt: Option<String>,
        access_mode: AccessMode,
    ) -> Self {
        let access_policy = AccessPolicy::new(&workspace, &actor, access_mode);
        Self::new(
            workspace,
            actor,
            session_plan,
            initial_prompt,
            access_policy,
        )
    }

    /// Bind a launch request to one immutable workspace and initiating actor.
    #[must_use]
    pub fn new(
        workspace: Arc<WorkspaceContext>,
        actor: ActorContext,
        session_plan: SessionPlan,
        initial_prompt: Option<String>,
        access_policy: AccessPolicy,
    ) -> Self {
        let channel = actor.channel();
        Self {
            workspace,
            actor,
            session_plan,
            initial_prompt,
            access_policy,
            channel,
            hook_metadata: HookMetadata::none(),
        }
    }

    /// Add trusted launch metadata consumed by lifecycle hooks.
    #[must_use]
    pub fn with_hook_metadata(mut self, hook_metadata: HookMetadata) -> Self {
        self.hook_metadata = hook_metadata;
        self
    }

    /// Attach a separately resolved workspace capability selection.
    #[must_use]
    pub fn with_capability_plan(mut self, plan: crate::access::CapabilityPlan) -> Self {
        self.access_policy = self.access_policy.with_capability_plan(plan);
        self
    }

    /// The selected workspace for this launch.
    #[must_use]
    pub fn workspace(&self) -> &Arc<WorkspaceContext> {
        &self.workspace
    }

    /// The immutable initiating actor.
    #[must_use]
    pub const fn actor(&self) -> &ActorContext {
        &self.actor
    }

    /// The fresh or resumed session choice.
    #[must_use]
    pub const fn session_plan(&self) -> &SessionPlan {
        &self.session_plan
    }

    /// Optional initial prompt supplied at launch.
    #[must_use]
    pub fn initial_prompt(&self) -> Option<&str> {
        self.initial_prompt.as_deref()
    }

    /// The policy carried by this launch.
    #[must_use]
    pub const fn access_policy(&self) -> &AccessPolicy {
        &self.access_policy
    }

    /// The initiating request channel, derived from the actor context.
    #[must_use]
    pub const fn channel(&self) -> Channel {
        self.channel
    }

    /// Trusted lifecycle-hook metadata for this launch.
    #[must_use]
    pub const fn hook_metadata(&self) -> &HookMetadata {
        &self.hook_metadata
    }
}

/// A complete, frontend-specific launch description for a transport.
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LaunchSpec {
    /// Executable shell command, including frontend-specific arguments.
    pub command: String,
    /// Working directory applied before the child process starts.
    pub cwd: PathBuf,
    /// Explicit minimal environment passed to the frontend child.
    pub environment: Vec<(String, String)>,
    /// Frontend-owned hook association metadata.
    pub hooks: HookMetadata,
    /// Honest per-capability enforcement derived from concrete launch flags.
    pub capabilities: crate::access::CapabilityEnforcementReport,
}

impl std::fmt::Debug for LaunchSpec {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LaunchSpec")
            .field("command", &"<redacted>")
            .field("cwd", &self.cwd)
            .field(
                "environment_keys",
                &self
                    .environment
                    .iter()
                    .map(|(name, _)| name)
                    .collect::<Vec<_>>(),
            )
            .field("hooks", &self.hooks)
            .field("capabilities", &self.capabilities)
            .finish()
    }
}

impl LaunchSpec {
    /// Construct the complete transport launch description.
    #[must_use]
    pub(crate) fn new(
        command: impl Into<String>,
        cwd: PathBuf,
        environment: Vec<(String, String)>,
        hooks: HookMetadata,
    ) -> Self {
        Self {
            command: command.into(),
            cwd,
            environment,
            hooks,
            capabilities: crate::access::CapabilityEnforcementReport::default(),
        }
    }

    /// Attach the report proven by this frontend's launch arguments.
    #[must_use]
    pub fn with_capabilities(
        mut self,
        capabilities: crate::access::CapabilityEnforcementReport,
    ) -> Self {
        self.capabilities = capabilities;
        self
    }
}

/// A concrete agent frontend translated behind the shared controller.
pub(crate) trait AgentFrontend: Send {
    /// This frontend's stable kind.
    fn kind(&self) -> AgentKind;

    /// Reject operations for a constructible but unavailable frontend.
    fn ensure_available(&self) -> Result<(), AgentError> {
        Ok(())
    }

    /// Translate a neutral launch request into a complete launch spec.
    fn launch_spec(&self, request: &LaunchRequest) -> Result<LaunchSpec, AgentError>;

    /// Remove frontend-owned artifacts prepared for a child that failed to start.
    fn rollback_launch(&self, _request: &LaunchRequest) -> Result<(), AgentError> {
        Ok(())
    }

    /// Translate one semantic input action into an atomic terminal sequence.
    fn input_for(&self, action: AgentAction<'_>) -> Result<InputSequence, AgentError>;

    /// Completion mechanism used by this frontend.
    fn completion_strategy(&self) -> Result<CompletionStrategy, AgentError>;

    /// Whether this known session is safe to offer as a resume candidate.
    fn resume_candidate_exists(&self, session: &AgentSession) -> Result<bool, AgentError>;

    /// Stable response artifact identity for a launched session.
    fn response_id(&self, session: &AgentSession) -> Result<String, AgentError>;

    /// Whether a completed receiver frontend session can restore interactive work.
    fn can_resume_response_session(&self, session: &AgentSession) -> Result<bool, AgentError>;
}

pub(crate) fn shell_quote(value: &str) -> String {
    let single_quote_bytes = value
        .len()
        .saturating_add(
            value
                .bytes()
                .filter(|byte| *byte == b'\'')
                .count()
                .saturating_mul(3),
        )
        .saturating_add(2);
    let double_quote_bytes = value
        .len()
        .saturating_add(
            value
                .bytes()
                .filter(|byte| matches!(*byte, b'\\' | b'"' | b'$' | b'`'))
                .count(),
        )
        .saturating_add(2);

    if double_quote_bytes < single_quote_bytes {
        let mut quoted = String::with_capacity(double_quote_bytes);
        quoted.push('"');
        for character in value.chars() {
            if matches!(character, '\\' | '"' | '$' | '`') {
                quoted.push('\\');
            }
            quoted.push(character);
        }
        quoted.push('"');
        return quoted;
    }

    let escaped = value.replace('\'', "'\\''");
    format!("'{escaped}'")
}

pub(crate) const fn shell_command_is_transport_safe(command: &str) -> bool {
    command.len() <= SHELL_COMMAND_ARGUMENT_BUDGET_BYTES
}

pub(super) fn launch_environment(
    request: &LaunchRequest,
    kind: AgentKind,
) -> Vec<(String, String)> {
    const FRONTEND_NECESSITIES: [&str; 10] = [
        "HOME",
        "PATH",
        "SHELL",
        "USER",
        "LOGNAME",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
        "TMPDIR",
        "SSH_AUTH_SOCK",
    ];
    let mut environment = FRONTEND_NECESSITIES
        .into_iter()
        .filter_map(|name| {
            std::env::var(name)
                .ok()
                .map(|value| (name.to_owned(), value))
        })
        .collect::<Vec<_>>();
    environment.extend(ambient_frontend_environment(kind, std::env::vars()));
    environment.extend(
        request
            .workspace()
            .integration_env(request.actor())
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value)),
    );
    environment.push(("BRAIN_AGENT_KIND".to_owned(), kind.as_str().to_owned()));
    environment.extend(request.hook_metadata().values().iter().cloned());
    environment
}

fn ambient_frontend_environment(
    kind: AgentKind,
    ambient: impl IntoIterator<Item = (String, String)>,
) -> Vec<(String, String)> {
    if kind != AgentKind::OpenCode {
        return Vec::new();
    }
    ambient
        .into_iter()
        .filter(|(name, _)| name.starts_with("OPENCODE_"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opencode_preserves_its_documented_environment_namespace_only_for_opencode() {
        let ambient = vec![
            (
                "OPENCODE_CONFIG".to_owned(),
                "/tmp/opencode.json".to_owned(),
            ),
            ("OPENCODE_CONFIG_DIR".to_owned(), "/tmp/opencode".to_owned()),
            ("OPENCODE_TUI_CONFIG".to_owned(), "/tmp/tui.json".to_owned()),
            ("OPENCODE_DISABLE_AUTOUPDATE".to_owned(), "true".to_owned()),
            ("UNRELATED_SECRET".to_owned(), "do-not-copy".to_owned()),
        ];

        let opencode = ambient_frontend_environment(AgentKind::OpenCode, ambient.clone());
        let claude = ambient_frontend_environment(AgentKind::Claude, ambient);

        assert_eq!(
            opencode,
            vec![
                (
                    "OPENCODE_CONFIG".to_owned(),
                    "/tmp/opencode.json".to_owned()
                ),
                ("OPENCODE_CONFIG_DIR".to_owned(), "/tmp/opencode".to_owned()),
                ("OPENCODE_TUI_CONFIG".to_owned(), "/tmp/tui.json".to_owned()),
                ("OPENCODE_DISABLE_AUTOUPDATE".to_owned(), "true".to_owned()),
            ]
        );
        assert!(claude.is_empty());
    }
}
