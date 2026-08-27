use std::error::Error;
use std::fmt::{Display, Formatter};

use uuid::Uuid;

/// Verified provider lineage, or an explicit request for a fresh email conversation.
#[derive(Clone, PartialEq, Eq)]
pub enum EmailLineage {
    /// A provider-authenticated thread identifier.
    Verified(String),
    /// No unambiguous provider lineage was available.
    Uncertain,
}

impl std::fmt::Debug for EmailLineage {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Verified(_) => "EmailLineage::Verified(<redacted>)",
            Self::Uncertain => "EmailLineage::Uncertain",
        })
    }
}

impl EmailLineage {
    /// Validate one provider-authenticated thread identifier.
    ///
    /// # Errors
    ///
    /// Returns [`EmailLineageError`] when the identifier is blank.
    pub fn verified(value: impl Into<String>) -> Result<Self, EmailLineageError> {
        let value = value.into();
        let value = value.trim();
        if value.is_empty() {
            return Err(EmailLineageError);
        }
        Ok(Self::Verified(value.to_owned()))
    }
}

/// A verified email lineage must contain a non-blank provider identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmailLineageError;

impl Display for EmailLineageError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("verified email lineage cannot be blank")
    }
}

impl Error for EmailLineageError {}

/// Immutable logical identity for one receiver conversation.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ReceiverConversationIdentity {
    workspace_id: crate::workspace::WorkspaceId,
    user_id: crate::users::UserId,
    channel: crate::server::receiver::Channel,
    conversation_key: String,
}

impl std::fmt::Debug for ReceiverConversationIdentity {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiverConversationIdentity(<redacted>)")
    }
}

impl ReceiverConversationIdentity {
    /// Select the single SMS conversation for a workspace and portable user.
    #[must_use]
    pub fn sms(workspace_id: crate::workspace::WorkspaceId, user_id: crate::users::UserId) -> Self {
        Self {
            workspace_id,
            user_id,
            channel: crate::server::receiver::Channel::Sms,
            conversation_key: "sms".to_owned(),
        }
    }

    /// Select a verified email thread or mint a fresh identity for uncertain lineage.
    #[must_use]
    pub fn email(
        workspace_id: crate::workspace::WorkspaceId,
        user_id: crate::users::UserId,
        lineage: EmailLineage,
    ) -> Self {
        let conversation_key = match lineage {
            EmailLineage::Verified(thread_id) => format!("thread:{thread_id}"),
            EmailLineage::Uncertain => format!("fresh:{}", Uuid::new_v4()),
        };
        Self {
            workspace_id,
            user_id,
            channel: crate::server::receiver::Channel::Email,
            conversation_key,
        }
    }

    /// Workspace that owns this conversation.
    #[must_use]
    pub const fn workspace_id(&self) -> crate::workspace::WorkspaceId {
        self.workspace_id
    }

    /// Portable user whose receiver lineage this conversation represents.
    #[must_use]
    pub const fn user_id(&self) -> &crate::users::UserId {
        &self.user_id
    }

    /// Authenticated receiver channel for this conversation.
    #[must_use]
    pub const fn channel(&self) -> crate::server::receiver::Channel {
        self.channel
    }

    pub(super) fn conversation_key(&self) -> &str {
        &self.conversation_key
    }

    pub(super) fn from_stored_parts(
        workspace_id: crate::workspace::WorkspaceId,
        user_id: crate::users::UserId,
        channel: crate::server::receiver::Channel,
        conversation_key: String,
    ) -> Self {
        Self {
            workspace_id,
            user_id,
            channel,
            conversation_key,
        }
    }
}
