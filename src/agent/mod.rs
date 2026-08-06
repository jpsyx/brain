//! Frontend-neutral control of interactive coding agents.
//!
//! The facade accepts semantic operations such as typing, submitting, and
//! queueing work. Frontends translate those operations into their own launch,
//! input, hook, and transcript conventions; transports own the PTY details.

mod claude;
mod codex;
mod controller;
pub mod frontend;
pub mod hooks;
pub mod input;
mod opencode;
pub mod session;

use std::{
    error::Error,
    fmt::{Display, Formatter},
};

pub use crate::access::AccessPolicy;
pub use claude::ClaudeFrontend;
pub use codex::CodexFrontend;
pub use controller::{AgentController, AgentTransport};
pub use frontend::{AgentFrontend, LaunchRequest, LaunchSpec};
pub use hooks::HookMetadata;
pub use input::InputSequence;
pub use opencode::OpenCodeFrontend;
pub use session::{
    AgentKind, AgentSession, CompletionStatus, CompletionStrategy, SessionPlan, SessionScope,
    SessionStore,
};

pub(crate) use claude::DEFAULT_COMMAND as DEFAULT_CLAUDE_COMMAND;
pub(crate) use claude::project_dir_name as claude_project_dir_name;
pub(crate) use codex::DEFAULT_COMMAND as DEFAULT_CODEX_COMMAND;
pub(crate) use opencode::DEFAULT_COMMAND as DEFAULT_OPENCODE_COMMAND;

pub(crate) fn configured_command(
    command: &crate::workspace::CommandContext,
    kind: AgentKind,
) -> String {
    match kind {
        AgentKind::Claude => crate::env::resolve_one(command, "claude_cmd")
            .unwrap_or_else(|| DEFAULT_CLAUDE_COMMAND.to_owned()),
        AgentKind::Codex => crate::env::resolve_one(command, "codex_cmd")
            .unwrap_or_else(|| DEFAULT_CODEX_COMMAND.to_owned()),
        AgentKind::OpenCode => crate::env::resolve_one(command, "opencode_cmd")
            .unwrap_or_else(|| DEFAULT_OPENCODE_COMMAND.to_owned()),
    }
}

pub(crate) fn configured_frontend(
    command: &crate::workspace::CommandContext,
    kind: AgentKind,
) -> Box<dyn AgentFrontend> {
    let configured = configured_command(command, kind);
    match kind {
        AgentKind::Claude => {
            let workspace_root = command.workspace.root().to_path_buf();
            if let Some(home) = std::env::var_os("HOME") {
                Box::new(ClaudeFrontend::new(
                    configured,
                    workspace_root,
                    std::path::PathBuf::from(home)
                        .join(".claude")
                        .join("projects"),
                ))
            } else {
                Box::new(ClaudeFrontend::without_projects_dir(
                    configured,
                    workspace_root,
                ))
            }
        }
        AgentKind::Codex => Box::new(CodexFrontend::new(configured)),
        AgentKind::OpenCode => Box::new(OpenCodeFrontend::new(configured)),
    }
}

pub(crate) fn build_command(
    kind: AgentKind,
    configured_command: &str,
    plan: &SessionPlan,
    prompt: Option<&str>,
) -> Result<String, AgentError> {
    match kind {
        AgentKind::Claude => Ok(ClaudeFrontend::command_for(
            configured_command,
            plan,
            prompt,
        )),
        AgentKind::Codex => Ok(CodexFrontend::command_for(configured_command, plan, prompt)),
        AgentKind::OpenCode => Err(AgentError::UnsupportedFrontend(AgentKind::OpenCode)),
    }
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
