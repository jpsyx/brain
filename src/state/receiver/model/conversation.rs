use std::error::Error;
use std::fmt::{Display, Formatter};

use super::ReceiverConversationId;
use crate::state::ReceiverConversationIdentity;

/// Current frontend-owned native session attached to a logical conversation.
#[derive(Clone, PartialEq, Eq)]
pub struct ReceiverSessionBinding {
    frontend: crate::agent::AgentKind,
    native_session_id: String,
}

impl std::fmt::Debug for ReceiverSessionBinding {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiverSessionBinding(<redacted>)")
    }
}

impl ReceiverSessionBinding {
    /// Validate one frontend/native-session pair.
    ///
    /// # Errors
    ///
    /// Returns [`ReceiverSessionBindingError`] when the session ID is blank.
    pub fn new(
        frontend: crate::agent::AgentKind,
        native_session_id: impl Into<String>,
    ) -> Result<Self, ReceiverSessionBindingError> {
        let native_session_id = native_session_id.into();
        let native_session_id = native_session_id.trim();
        if native_session_id.is_empty() {
            return Err(ReceiverSessionBindingError);
        }
        Ok(Self {
            frontend,
            native_session_id: native_session_id.to_owned(),
        })
    }

    /// Choose native resume only when the requested frontend owns the binding.
    #[must_use]
    pub fn plan(
        &self,
        requested: crate::agent::AgentKind,
        transcript_markdown: &str,
    ) -> ReceiverSessionPlan {
        if requested == self.frontend {
            ReceiverSessionPlan::ResumeNative(self.native_session_id.clone())
        } else {
            ReceiverSessionPlan::FreshFromTranscript(transcript_markdown.to_owned())
        }
    }

    pub(crate) const fn frontend(&self) -> crate::agent::AgentKind {
        self.frontend
    }

    pub(crate) fn native_session_id(&self) -> &str {
        &self.native_session_id
    }
}

/// A native receiver session binding requires a non-blank ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReceiverSessionBindingError;

impl Display for ReceiverSessionBindingError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("receiver native session ID cannot be blank")
    }
}

impl Error for ReceiverSessionBindingError {}

/// Session-continuity decision for the next isolated receiver run.
#[derive(Clone, PartialEq, Eq)]
pub enum ReceiverSessionPlan {
    ResumeNative(String),
    FreshFromTranscript(String),
}

impl std::fmt::Debug for ReceiverSessionPlan {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::ResumeNative(_) => "ReceiverSessionPlan::ResumeNative(<redacted>)",
            Self::FreshFromTranscript(_) => "ReceiverSessionPlan::FreshFromTranscript(<redacted>)",
        })
    }
}

/// One persisted logical receiver conversation.
#[derive(Clone, PartialEq, Eq)]
pub struct ReceiverConversation {
    id: ReceiverConversationId,
    identity: ReceiverConversationIdentity,
    transcript_markdown: String,
    binding: Option<ReceiverSessionBinding>,
}

impl std::fmt::Debug for ReceiverConversation {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiverConversation(<redacted>)")
    }
}

impl ReceiverConversation {
    pub(in crate::state::receiver) fn from_stored(
        id: ReceiverConversationId,
        identity: ReceiverConversationIdentity,
        transcript_markdown: String,
        binding: Option<ReceiverSessionBinding>,
    ) -> Self {
        Self {
            id,
            identity,
            transcript_markdown,
            binding,
        }
    }

    #[must_use]
    pub const fn id(&self) -> ReceiverConversationId {
        self.id
    }
    #[must_use]
    pub const fn identity(&self) -> &ReceiverConversationIdentity {
        &self.identity
    }
    #[must_use]
    pub fn transcript_markdown(&self) -> &str {
        &self.transcript_markdown
    }
    #[must_use]
    pub const fn binding(&self) -> Option<&ReceiverSessionBinding> {
        self.binding.as_ref()
    }
    #[must_use]
    pub fn session_plan(&self, requested: crate::agent::AgentKind) -> ReceiverSessionPlan {
        self.binding.as_ref().map_or_else(
            || ReceiverSessionPlan::FreshFromTranscript(self.transcript_markdown.clone()),
            |binding| binding.plan(requested, &self.transcript_markdown),
        )
    }
}
