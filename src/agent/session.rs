//! Frontend-neutral session identity and lifecycle choices.

use anyhow::Result;

use crate::agent::AgentError;

/// Which agent frontend the brain panel is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    /// Claude Code.
    Claude,
    /// OpenAI Codex.
    Codex,
}

impl AgentKind {
    /// Human label for UI copy.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
        }
    }

    /// Stable state-database representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}

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

/// Immutable lookup scope for one actor's sessions in one workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionScope {
    agent_kind: AgentKind,
    workspace_id: crate::workspace::WorkspaceId,
    actor: crate::actor::ActorContext,
}

impl SessionScope {
    /// Bind persistence operations to one frontend, workspace, and actor lineage.
    #[must_use]
    pub const fn new(
        agent_kind: AgentKind,
        workspace_id: crate::workspace::WorkspaceId,
        actor: crate::actor::ActorContext,
    ) -> Self {
        Self {
            agent_kind,
            workspace_id,
            actor,
        }
    }

    /// Frontend whose opaque session namespace this scope uses.
    #[must_use]
    pub const fn agent_kind(&self) -> AgentKind {
        self.agent_kind
    }

    /// Workspace that owns the session.
    #[must_use]
    pub const fn workspace_id(&self) -> crate::workspace::WorkspaceId {
        self.workspace_id
    }

    /// Actor and initiating channel attributed to the session.
    #[must_use]
    pub const fn actor(&self) -> &crate::actor::ActorContext {
        &self.actor
    }
}

/// Durable completion lifecycle for a registered frontend session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionStatus {
    /// The session may still produce a turn completion.
    Active,
    /// A registered completion event has finished the current turn.
    Completed,
}

impl CompletionStatus {
    /// Stable state-database representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Completed => "completed",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "completed" => Some(Self::Completed),
            _ => None,
        }
    }
}

/// Frontend-neutral persistence used by every live agent controller.
pub trait SessionStore {
    /// Reap locks whose owning shell has exited.
    fn reap_dead_locks(&self) -> Result<()>;

    /// List free sessions newest first within an immutable scope.
    fn sessions_by_recency(&self, scope: &SessionScope) -> Vec<String>;

    /// Claim one free session for a shell.
    fn claim(
        &self,
        session: &AgentSession,
        instance: &str,
        pid: i32,
        scope: &SessionScope,
    ) -> Result<bool>;

    /// Register a fresh, active session with complete attribution.
    fn register(
        &self,
        session: &AgentSession,
        instance: &str,
        pid: i32,
        scope: &SessionScope,
    ) -> Result<()>;

    /// Release every session held by a shell instance.
    fn release(&self, instance: &str) -> Result<()>;

    /// Mark the currently locked session in one shell lineage active.
    fn mark_active(&self, instance: &str, scope: &SessionScope) -> Result<bool>;

    /// Mark an exactly scoped session completed.
    fn mark_completed(&self, session: &AgentSession, scope: &SessionScope) -> Result<bool>;

    /// Read the completion status for an exactly scoped session.
    fn completion_status(
        &self,
        session: &AgentSession,
        scope: &SessionScope,
    ) -> Option<CompletionStatus>;
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
