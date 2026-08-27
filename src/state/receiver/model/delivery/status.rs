use serde::{Deserialize, Serialize};

use super::{ReceiverDeliveryId, ReceiverResponseKind};

/// Durable phase of one response delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReceiverDeliveryState {
    Ready,
    Delivering,
    Retrying,
    Acknowledged,
    Failed,
    Ambiguous,
}

impl ReceiverDeliveryState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Delivering => "delivering",
            Self::Retrying => "retrying",
            Self::Acknowledged => "acknowledged",
            Self::Failed => "failed",
            Self::Ambiguous => "ambiguous",
        }
    }
}

/// Stable, content-free delivery failure category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReceiverDeliveryErrorCategory {
    Authorization,
    Credentials,
    InvalidRequest,
    ProviderRejected,
    TransportUnavailable,
    RetryExhausted,
    IdempotencyWindowExpired,
}

impl ReceiverDeliveryErrorCategory {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Authorization => "authorization",
            Self::Credentials => "credentials",
            Self::InvalidRequest => "invalid-request",
            Self::ProviderRejected => "provider-rejected",
            Self::TransportUnavailable => "transport-unavailable",
            Self::RetryExhausted => "retry-exhausted",
            Self::IdempotencyWindowExpired => "idempotency-window-expired",
        }
    }
}

/// Stable reason that provider acceptance cannot be determined safely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReceiverDeliveryAmbiguity {
    ProviderAcceptanceUnknown,
    ProviderAcknowledgementMalformed,
    ResultCommitUnknown,
    IdempotencyWindowExpired,
}

impl ReceiverDeliveryAmbiguity {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProviderAcceptanceUnknown => "provider-acceptance-unknown",
            Self::ProviderAcknowledgementMalformed => "provider-acknowledgement-malformed",
            Self::ResultCommitUnknown => "result-commit-unknown",
            Self::IdempotencyWindowExpired => "idempotency-window-expired",
        }
    }
}

/// Content-free attempt and retry timing for one delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReceiverDeliveryRetryMetadata {
    attempt_count: u32,
    retry_at_unix_ms: Option<u64>,
    first_attempt_at_unix_ms: Option<u64>,
}

impl ReceiverDeliveryRetryMetadata {
    #[must_use]
    pub const fn new(
        attempt_count: u32,
        retry_at_unix_ms: Option<u64>,
        first_attempt_at_unix_ms: Option<u64>,
    ) -> Self {
        Self {
            attempt_count,
            retry_at_unix_ms,
            first_attempt_at_unix_ms,
        }
    }

    #[must_use]
    pub const fn attempt_count(self) -> u32 {
        self.attempt_count
    }

    #[must_use]
    pub const fn retry_at_unix_ms(self) -> Option<u64> {
        self.retry_at_unix_ms
    }

    #[must_use]
    pub const fn first_attempt_at_unix_ms(self) -> Option<u64> {
        self.first_attempt_at_unix_ms
    }
}

/// Content-free delivery status safe for CLI and diagnostic surfaces.
#[derive(Clone, PartialEq, Eq)]
pub struct ReceiverDeliveryStatus {
    delivery_id: ReceiverDeliveryId,
    response_kind: ReceiverResponseKind,
    state: ReceiverDeliveryState,
    attempt_count: u32,
    retry_at_unix_ms: Option<u64>,
    error_category: Option<ReceiverDeliveryErrorCategory>,
    ambiguity: Option<ReceiverDeliveryAmbiguity>,
    has_provider_reference: bool,
}

impl ReceiverDeliveryStatus {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn new(
        delivery_id: ReceiverDeliveryId,
        response_kind: ReceiverResponseKind,
        state: ReceiverDeliveryState,
        attempt_count: u32,
        retry_at_unix_ms: Option<u64>,
        error_category: Option<ReceiverDeliveryErrorCategory>,
        ambiguity: Option<ReceiverDeliveryAmbiguity>,
        has_provider_reference: bool,
    ) -> Self {
        Self {
            delivery_id,
            response_kind,
            state,
            attempt_count,
            retry_at_unix_ms,
            error_category,
            ambiguity,
            has_provider_reference,
        }
    }

    #[must_use]
    pub const fn delivery_id(&self) -> ReceiverDeliveryId {
        self.delivery_id
    }

    #[must_use]
    pub const fn response_kind(&self) -> ReceiverResponseKind {
        self.response_kind
    }

    #[must_use]
    pub const fn state(&self) -> ReceiverDeliveryState {
        self.state
    }

    #[must_use]
    pub const fn attempt_count(&self) -> u32 {
        self.attempt_count
    }

    #[must_use]
    pub const fn retry_at_unix_ms(&self) -> Option<u64> {
        self.retry_at_unix_ms
    }

    #[must_use]
    pub const fn error_category(&self) -> Option<ReceiverDeliveryErrorCategory> {
        self.error_category
    }

    #[must_use]
    pub const fn ambiguity(&self) -> Option<ReceiverDeliveryAmbiguity> {
        self.ambiguity
    }

    #[must_use]
    pub const fn has_provider_reference(&self) -> bool {
        self.has_provider_reference
    }
}

impl std::fmt::Debug for ReceiverDeliveryStatus {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiverDeliveryStatus(<redacted>)")
    }
}
