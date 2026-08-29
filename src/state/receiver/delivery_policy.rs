use super::{ReceiverDeliveryAmbiguity, ReceiverDeliveryErrorCategory, ReceiverProviderReference};

const RETRY_DELAYS_UNIX_MS: [u64; 3] = [60_000, 300_000, 1_800_000];
const RESEND_IDEMPOTENCY_WINDOW_UNIX_MS: u64 = 24 * 60 * 60 * 1_000;

/// External provider replay capability for one immutable envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReceiverProviderCapability {
    Twilio,
    Resend,
}

/// Redacted classification of one provider attempt result.
#[derive(Clone, PartialEq, Eq)]
pub enum ReceiverProviderResultClass {
    Acknowledged(ReceiverProviderReference),
    DefinitelyNotAccepted(ReceiverDeliveryErrorCategory),
    PermanentlyRejected(ReceiverDeliveryErrorCategory),
    Ambiguous(ReceiverDeliveryAmbiguity),
}

impl std::fmt::Debug for ReceiverProviderResultClass {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Acknowledged(_) => formatter.write_str("Acknowledged(<redacted>)"),
            Self::DefinitelyNotAccepted(category) => formatter
                .debug_tuple("DefinitelyNotAccepted")
                .field(category)
                .finish(),
            Self::PermanentlyRejected(category) => formatter
                .debug_tuple("PermanentlyRejected")
                .field(category)
                .finish(),
            Self::Ambiguous(reason) => formatter.debug_tuple("Ambiguous").field(reason).finish(),
        }
    }
}

/// Clock-injected facts needed to classify one completed attempt.
#[derive(Clone, PartialEq, Eq)]
pub struct ReceiverDeliveryPolicySnapshot {
    pub provider: ReceiverProviderCapability,
    pub attempt_count: u32,
    pub first_attempt_at_unix_ms: Option<u64>,
    pub now_unix_ms: u64,
    pub result: ReceiverProviderResultClass,
}

impl std::fmt::Debug for ReceiverDeliveryPolicySnapshot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiverDeliveryPolicySnapshot(<redacted>)")
    }
}

/// Durable next step after one provider result.
#[derive(Clone, PartialEq, Eq)]
pub enum ReceiverDeliveryDecision {
    Acknowledged(ReceiverProviderReference),
    RetryAt {
        retry_at_unix_ms: u64,
        error_category: ReceiverDeliveryErrorCategory,
    },
    TerminalFailure(ReceiverDeliveryErrorCategory),
    TerminalAmbiguous(ReceiverDeliveryAmbiguity),
}

impl std::fmt::Debug for ReceiverDeliveryDecision {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Acknowledged(_) => formatter.write_str("Acknowledged(<redacted>)"),
            Self::RetryAt {
                retry_at_unix_ms,
                error_category,
            } => formatter
                .debug_struct("RetryAt")
                .field("retry_at_unix_ms", retry_at_unix_ms)
                .field("error_category", error_category)
                .finish(),
            Self::TerminalFailure(category) => formatter
                .debug_tuple("TerminalFailure")
                .field(category)
                .finish(),
            Self::TerminalAmbiguous(reason) => formatter
                .debug_tuple("TerminalAmbiguous")
                .field(reason)
                .finish(),
        }
    }
}

/// Decide the durable result of one provider attempt without reading wall time.
#[must_use]
pub fn decide_receiver_delivery(
    snapshot: ReceiverDeliveryPolicySnapshot,
) -> ReceiverDeliveryDecision {
    if snapshot.attempt_count == 0 {
        return ReceiverDeliveryDecision::TerminalFailure(
            ReceiverDeliveryErrorCategory::InvalidRequest,
        );
    }
    match snapshot.result {
        ReceiverProviderResultClass::Acknowledged(reference) => {
            ReceiverDeliveryDecision::Acknowledged(reference)
        }
        ReceiverProviderResultClass::PermanentlyRejected(category) => {
            ReceiverDeliveryDecision::TerminalFailure(category)
        }
        ReceiverProviderResultClass::DefinitelyNotAccepted(category) => {
            if category == ReceiverDeliveryErrorCategory::TransportUnavailable {
                retry_decision(snapshot.attempt_count, snapshot.now_unix_ms, category)
            } else {
                ReceiverDeliveryDecision::TerminalFailure(category)
            }
        }
        ReceiverProviderResultClass::Ambiguous(reason) => decide_ambiguity(&snapshot, reason),
    }
}

fn decide_ambiguity(
    snapshot: &ReceiverDeliveryPolicySnapshot,
    reason: ReceiverDeliveryAmbiguity,
) -> ReceiverDeliveryDecision {
    if snapshot.provider == ReceiverProviderCapability::Twilio {
        return ReceiverDeliveryDecision::TerminalAmbiguous(reason);
    }
    let Some(first_attempt_at_unix_ms) = snapshot.first_attempt_at_unix_ms else {
        return ReceiverDeliveryDecision::TerminalAmbiguous(reason);
    };
    let idempotency_deadline =
        first_attempt_at_unix_ms.saturating_add(RESEND_IDEMPOTENCY_WINDOW_UNIX_MS);
    if snapshot.now_unix_ms > idempotency_deadline {
        return ReceiverDeliveryDecision::TerminalAmbiguous(
            ReceiverDeliveryAmbiguity::IdempotencyWindowExpired,
        );
    }
    match retry_decision(
        snapshot.attempt_count,
        snapshot.now_unix_ms,
        ReceiverDeliveryErrorCategory::TransportUnavailable,
    ) {
        ReceiverDeliveryDecision::RetryAt {
            retry_at_unix_ms, ..
        } if retry_at_unix_ms > idempotency_deadline => {
            ReceiverDeliveryDecision::TerminalAmbiguous(
                ReceiverDeliveryAmbiguity::IdempotencyWindowExpired,
            )
        }
        ReceiverDeliveryDecision::TerminalFailure(_) => {
            ReceiverDeliveryDecision::TerminalAmbiguous(reason)
        }
        decision => decision,
    }
}

fn retry_decision(
    attempt_count: u32,
    now_unix_ms: u64,
    error_category: ReceiverDeliveryErrorCategory,
) -> ReceiverDeliveryDecision {
    let retry_index = usize::try_from(attempt_count.saturating_sub(1)).unwrap_or(usize::MAX);
    let Some(delay) = RETRY_DELAYS_UNIX_MS.get(retry_index) else {
        return ReceiverDeliveryDecision::TerminalFailure(
            ReceiverDeliveryErrorCategory::RetryExhausted,
        );
    };
    ReceiverDeliveryDecision::RetryAt {
        retry_at_unix_ms: now_unix_ms.saturating_add(*delay),
        error_category,
    }
}

/// A persisted retry is due at or after its exact deadline.
#[must_use]
pub const fn receiver_delivery_retry_is_due(now_unix_ms: u64, retry_at_unix_ms: u64) -> bool {
    now_unix_ms >= retry_at_unix_ms
}

/// Whether replaying one persisted retry would exceed Resend's exact key lifetime.
#[must_use]
pub const fn receiver_delivery_replay_window_is_expired(
    provider: ReceiverProviderCapability,
    attempt_count: u32,
    first_attempt_at_unix_ms: Option<u64>,
    now_unix_ms: u64,
) -> bool {
    if !matches!(provider, ReceiverProviderCapability::Resend) || attempt_count == 0 {
        return false;
    }
    let Some(first_attempt_at_unix_ms) = first_attempt_at_unix_ms else {
        return false;
    };
    now_unix_ms > first_attempt_at_unix_ms.saturating_add(RESEND_IDEMPOTENCY_WINDOW_UNIX_MS)
}
