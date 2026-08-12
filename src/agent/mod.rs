//! Frontend-neutral control of interactive coding agents.
//!
//! The facade accepts semantic operations such as typing, submitting, and
//! queueing work. Frontends translate those operations into their own launch,
//! input, hook, and transcript conventions; transports own the PTY details.

mod claude;
mod codex;
mod controller;
pub mod default_frontend;
pub(crate) mod frontend;
pub mod hooks;
mod input;
mod opencode;
mod registry;
pub mod session;

use std::{
    error::Error,
    fmt::{Display, Formatter},
};

pub use crate::access::AccessPolicy;
pub use controller::{AgentController, AgentTransport};
pub use default_frontend::resolved_frontend;
pub use frontend::{LaunchRequest, LaunchSpec};
pub use hooks::HookMetadata;
pub use input::{InputSequence, InputWrite};
pub use session::{
    AgentKind, AgentSession, CompletionStatus, CompletionStrategy, SessionPlan, SessionScope,
    SessionStore,
};

pub(crate) use claude::ClaudeFrontend;
pub(crate) use claude::DEFAULT_COMMAND as DEFAULT_CLAUDE_COMMAND;
pub(crate) use claude::project_dir_name as claude_project_dir_name;
pub(crate) use codex::CodexFrontend;
pub(crate) use codex::DEFAULT_COMMAND as DEFAULT_CODEX_COMMAND;
pub(crate) use frontend::{AgentAction, AgentFrontend};
pub(crate) use opencode::DEFAULT_COMMAND as DEFAULT_OPENCODE_COMMAND;
pub(crate) use opencode::OpenCodeFrontend;
pub(crate) use opencode::compatibility_version as opencode_compatibility_version;
pub(crate) use registry::{
    HealthCheckDescriptor, HealthCheckExpectation, HookCommandStyle, LifecycleInstallation,
    LifecyclePayload, primary_session_health_check, registration, registrations,
};

pub(crate) fn configured_command(
    command: &crate::workspace::CommandContext,
    kind: AgentKind,
) -> String {
    registration(kind).configured_command(command)
}

pub(crate) fn configured_frontend_with_command(
    workspace: &crate::workspace::WorkspaceContext,
    kind: AgentKind,
    configured_command: String,
) -> Box<dyn AgentFrontend> {
    (registration(kind).frontend_constructor())(workspace, configured_command)
}

pub(crate) fn configured_frontend(
    command: &crate::workspace::CommandContext,
    kind: AgentKind,
) -> Box<dyn AgentFrontend> {
    registration(kind).frontend(command)
}

pub(crate) fn build_command(
    kind: AgentKind,
    configured_command: &str,
    plan: &SessionPlan,
    prompt: Option<&str>,
) -> String {
    registration(kind).build_command(configured_command, plan, prompt)
}

/// A facade, frontend, or transport operation could not complete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentError {
    /// A controller operation needs non-blank semantic text.
    EmptyInput,
    /// A frontend session identifier was blank.
    EmptySessionId,
    /// A launch request did not match its controller's immutable context.
    ContextMismatch,
    /// A frontend rejected an otherwise valid facade request.
    Frontend(String),
    /// A transport could not start or communicate with its child process.
    Transport(String),
    /// A selected frontend is known but intentionally unavailable.
    UnsupportedFrontend(AgentKind),
}

impl Display for AgentError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyInput => formatter.write_str("agent input cannot be blank"),
            Self::EmptySessionId => formatter.write_str("agent session id cannot be blank"),
            Self::ContextMismatch => {
                formatter.write_str("launch request does not match controller context")
            }
            Self::Frontend(message) => write!(formatter, "frontend error: {message}"),
            Self::Transport(message) => write!(formatter, "transport error: {message}"),
            Self::UnsupportedFrontend(kind) => {
                write!(formatter, "{} is not supported", kind.label())
            }
        }
    }
}

impl Error for AgentError {}

#[cfg(test)]
mod adapter_tests;
