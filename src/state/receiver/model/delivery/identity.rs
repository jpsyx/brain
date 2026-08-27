use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stable identity for one semantic response delivery.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReceiverDeliveryId(Uuid);

impl ReceiverDeliveryId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Parse one persisted delivery identity.
    ///
    /// # Errors
    ///
    /// Returns an error when `value` is not a UUID.
    pub fn parse(value: &str) -> anyhow::Result<Self> {
        Ok(Self(Uuid::parse_str(value)?))
    }
}

impl Default for ReceiverDeliveryId {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for ReceiverDeliveryId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::fmt::Debug for ReceiverDeliveryId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiverDeliveryId(<redacted>)")
    }
}

/// Exact identity for one provider attempt.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReceiverDeliveryAttemptId(Uuid);

impl ReceiverDeliveryAttemptId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Parse one persisted attempt identity.
    ///
    /// # Errors
    ///
    /// Returns an error when `value` is not a UUID.
    pub fn parse(value: &str) -> anyhow::Result<Self> {
        Ok(Self(Uuid::parse_str(value)?))
    }
}

impl Default for ReceiverDeliveryAttemptId {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for ReceiverDeliveryAttemptId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::fmt::Debug for ReceiverDeliveryAttemptId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiverDeliveryAttemptId(<redacted>)")
    }
}

/// Provider-owned acknowledgement identity with content-redacting diagnostics.
#[derive(Clone, PartialEq, Eq)]
pub struct ReceiverProviderReference(String);

impl ReceiverProviderReference {
    /// Parse one non-blank provider reference.
    ///
    /// # Errors
    ///
    /// Returns an error when `value` is blank.
    pub fn parse(value: impl Into<String>) -> anyhow::Result<Self> {
        let value = value.into();
        let trimmed = value.trim();
        anyhow::ensure!(!trimmed.is_empty(), "provider reference cannot be blank");
        Ok(Self(trimmed.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for ReceiverProviderReference {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiverProviderReference(<redacted>)")
    }
}

/// Semantic purpose of one delivery row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReceiverResponseKind {
    FinalAnswer,
    UnavailableNotice,
    ControlAcknowledgement,
    FallbackNotice,
}

impl ReceiverResponseKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FinalAnswer => "final-answer",
            Self::UnavailableNotice => "unavailable-notice",
            Self::ControlAcknowledgement => "control-acknowledgement",
            Self::FallbackNotice => "fallback-notice",
        }
    }
}
