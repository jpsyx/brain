//! Frontend-neutral session identity and lifecycle choices.

use crate::agent::AgentError;

pub use crate::session::AgentKind;

/// A non-blank session identifier assigned by an agent frontend.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AgentSession(String);

impl AgentSession {
    /// Validate and retain a frontend session identifier.
    ///
    /// # Errors
    ///
    /// Returns [`AgentError::EmptySessionId`] when `id` contains only
    /// whitespace.
    pub fn new(id: impl Into<String>) -> Result<Self, AgentError> {
        let id = id.into();
        let trimmed = id.trim();
        if trimmed.is_empty() {
            return Err(AgentError::EmptySessionId);
        }
        Ok(Self(trimmed.to_owned()))
    }

    /// The frontend's session identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_id_rejects_blank_values() {
        assert_eq!(AgentSession::new("  "), Err(AgentError::EmptySessionId));
    }
}

/// Whether to begin a new agent session or resume a specific existing one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionPlan {
    /// Start a new frontend session with this Brain-selected identifier.
    Fresh(AgentSession),
    /// Resume this known frontend session.
    Resume(AgentSession),
}

impl SessionPlan {
    /// Start a fresh agent session.
    #[must_use]
    pub const fn fresh(session: AgentSession) -> Self {
        Self::Fresh(session)
    }

    /// Resume an existing, validated agent session.
    #[must_use]
    pub const fn resume(session: AgentSession) -> Self {
        Self::Resume(session)
    }

    /// Choose a resumable session when available, otherwise start fresh.
    #[must_use]
    pub fn decide(resume_candidate: Option<AgentSession>, fresh: AgentSession) -> Self {
        resume_candidate.map_or(Self::Fresh(fresh), Self::Resume)
    }

    /// The selected frontend session identifier.
    #[must_use]
    pub const fn session(&self) -> &AgentSession {
        match self {
            Self::Fresh(session) | Self::Resume(session) => session,
        }
    }
}

/// How a frontend reports that a turn has completed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionStrategy {
    /// A frontend-owned hook notifies Brain of completion.
    Hook,
    /// The transport process ending signals completion.
    TransportExit,
}
