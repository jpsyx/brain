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

/// All frontend-neutral inputs required to launch an agent.
#[derive(Debug, Clone)]
pub struct LaunchRequest {
    workspace: Arc<WorkspaceContext>,
    actor: ActorContext,
    session_plan: SessionPlan,
    initial_prompt: Option<String>,
    access_policy: AccessPolicy,
    channel: Channel,
    hook_metadata: HookMetadata,
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
#[derive(Debug, Clone, PartialEq, Eq)]
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

impl LaunchSpec {
    /// Construct the complete transport launch description.
    #[must_use]
    pub fn new(
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
pub trait AgentFrontend: Send {
    /// This frontend's stable kind.
    fn kind(&self) -> AgentKind;

    /// Translate a neutral launch request into a complete launch spec.
    fn launch_spec(&self, request: &LaunchRequest) -> Result<LaunchSpec, AgentError>;

    /// Input sequence that immediately submits the current turn.
    fn submit_input(&self) -> InputSequence;

    /// Input sequence that queues text after the active turn.
    fn queue_input(&self) -> InputSequence;

    /// Input sequence that begins a fresh in-frontend session.
    fn new_session_input(&self) -> InputSequence;

    /// Completion mechanism used by this frontend.
    fn completion_strategy(&self) -> CompletionStrategy;

    /// Location of the frontend's transcript for a known session.
    fn transcript(&self, session: &AgentSession) -> Option<PathBuf>;

    /// Whether this known session is safe to offer as a resume candidate.
    fn resume_candidate_exists(&self, session: &AgentSession) -> bool;

    /// Stable response artifact identity for a launched session.
    fn response_id(&self, session: &AgentSession) -> String;

    /// Whether a completed receiver session ID can restore interactive work.
    fn can_resume_response_session(&self) -> bool;
}

pub(crate) fn shell_quote(value: &str) -> String {
    let escaped = value.replace('\'', "'\\''");
    format!("'{escaped}'")
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
