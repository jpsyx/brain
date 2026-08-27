use std::fmt::{Display, Formatter};

use uuid::Uuid;

/// Immutable identifier for one workspace-scoped receiver job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReceiverJobId(Uuid);

impl ReceiverJobId {
    pub(in crate::state::receiver) fn parse(value: &str) -> anyhow::Result<Self> {
        Ok(Self(Uuid::parse_str(value)?))
    }
}

impl From<Uuid> for ReceiverJobId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl Display for ReceiverJobId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Opaque correlation identity for the complete lifetime of one receiver job.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReceiverJobToken(Uuid);

impl ReceiverJobToken {
    pub(in crate::state::receiver) fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Parse a persisted opaque receiver job token.
    ///
    /// # Errors
    ///
    /// Returns an error when `value` is not a UUID token.
    pub fn parse(value: &str) -> anyhow::Result<Self> {
        Ok(Self(Uuid::parse_str(value)?))
    }
}

impl Display for ReceiverJobToken {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::fmt::Debug for ReceiverJobToken {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiverJobToken(<redacted>)")
    }
}

/// Immutable identifier for one workspace-scoped logical conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReceiverConversationId(Uuid);

impl ReceiverConversationId {
    pub(in crate::state::receiver) fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub(in crate::state::receiver) fn parse(value: &str) -> anyhow::Result<Self> {
        Ok(Self(Uuid::parse_str(value)?))
    }
}

impl Display for ReceiverConversationId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Exact durable attribution registered before one isolated receiver launch.
#[derive(Clone, PartialEq, Eq)]
pub struct ReceiverSessionAttribution {
    conversation_id: ReceiverConversationId,
    instance: String,
    registered_session: crate::agent::AgentSession,
    scope: crate::agent::SessionScope,
}

impl std::fmt::Debug for ReceiverSessionAttribution {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiverSessionAttribution(<redacted>)")
    }
}

impl ReceiverSessionAttribution {
    pub(in crate::state::receiver) fn new(
        conversation_id: ReceiverConversationId,
        instance: String,
        registered_session: crate::agent::AgentSession,
        scope: crate::agent::SessionScope,
    ) -> Self {
        Self {
            conversation_id,
            instance,
            registered_session,
            scope,
        }
    }

    #[must_use]
    pub const fn conversation_id(&self) -> ReceiverConversationId {
        self.conversation_id
    }
    #[must_use]
    pub fn instance(&self) -> &str {
        &self.instance
    }
    #[must_use]
    pub const fn registered_session(&self) -> &crate::agent::AgentSession {
        &self.registered_session
    }
    #[must_use]
    pub const fn scope(&self) -> &crate::agent::SessionScope {
        &self.scope
    }
}
