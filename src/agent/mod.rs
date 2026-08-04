//! Frontend-neutral control of interactive coding agents.
//!
//! The facade accepts semantic operations such as typing, submitting, and
//! queueing work. Frontends translate those operations into their own launch,
//! input, hook, and transcript conventions; transports own the PTY details.

mod controller;
pub mod frontend;
pub mod hooks;
pub mod input;
pub mod session;

use std::{
    error::Error,
    fmt::{Display, Formatter},
};

pub use controller::{AgentController, AgentTransport};
pub use frontend::{AccessPolicy, AgentFrontend, LaunchRequest, LaunchSpec};
pub use hooks::HookMetadata;
pub use input::InputSequence;
pub use session::{AgentKind, AgentSession, CompletionStrategy, SessionPlan};

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
