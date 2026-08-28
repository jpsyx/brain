use super::{ReceiverDeliveryAttemptId, ReceiverDeliveryEnvelope, ReceiverDeliveryId};
use crate::state::{ReceiverJobId, ReceiverJobToken, ReceiverProviderCapability};

/// Exact finite ownership of one immutable final-answer provider attempt.
#[derive(Clone, PartialEq, Eq)]
pub struct ReceiverDeliveryClaim {
    delivery_id: ReceiverDeliveryId,
    attempt_id: ReceiverDeliveryAttemptId,
    job_id: ReceiverJobId,
    token: ReceiverJobToken,
    owner: String,
    expires_at_unix_ms: u64,
    attempt_count: u32,
    first_attempt_at_unix_ms: Option<u64>,
    envelope: ReceiverDeliveryEnvelope,
}

impl ReceiverDeliveryClaim {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::state::receiver) fn new(
        delivery_id: ReceiverDeliveryId,
        attempt_id: ReceiverDeliveryAttemptId,
        job_id: ReceiverJobId,
        token: ReceiverJobToken,
        owner: String,
        expires_at_unix_ms: u64,
        attempt_count: u32,
        first_attempt_at_unix_ms: Option<u64>,
        envelope: ReceiverDeliveryEnvelope,
    ) -> Self {
        Self {
            delivery_id,
            attempt_id,
            job_id,
            token,
            owner,
            expires_at_unix_ms,
            attempt_count,
            first_attempt_at_unix_ms,
            envelope,
        }
    }

    #[must_use]
    pub const fn delivery_id(&self) -> ReceiverDeliveryId {
        self.delivery_id
    }

    #[must_use]
    pub const fn attempt_id(&self) -> ReceiverDeliveryAttemptId {
        self.attempt_id
    }

    #[must_use]
    pub const fn job_id(&self) -> ReceiverJobId {
        self.job_id
    }

    #[must_use]
    pub const fn token(&self) -> ReceiverJobToken {
        self.token
    }

    #[must_use]
    pub fn owner(&self) -> &str {
        &self.owner
    }

    #[must_use]
    pub const fn expires_at_unix_ms(&self) -> u64 {
        self.expires_at_unix_ms
    }

    #[must_use]
    pub const fn attempt_count(&self) -> u32 {
        self.attempt_count
    }

    #[must_use]
    pub const fn first_attempt_at_unix_ms(&self) -> Option<u64> {
        self.first_attempt_at_unix_ms
    }

    #[must_use]
    pub const fn envelope(&self) -> &ReceiverDeliveryEnvelope {
        &self.envelope
    }

    #[must_use]
    pub const fn provider(&self) -> ReceiverProviderCapability {
        match self.envelope {
            ReceiverDeliveryEnvelope::Sms { .. } => ReceiverProviderCapability::Twilio,
            ReceiverDeliveryEnvelope::Email { .. } => ReceiverProviderCapability::Resend,
        }
    }
}

impl std::fmt::Debug for ReceiverDeliveryClaim {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiverDeliveryClaim(<redacted>)")
    }
}

/// Whether an exact provider result won its durable compare-and-swap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverDeliveryApplyOutcome {
    Applied,
    Stale,
}
