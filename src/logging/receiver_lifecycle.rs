use std::fmt::{Display, Write as _};

/// Finite durable receiver lifecycle phase allowed in logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReceiverLifecyclePhase {
    Queued,
    Claimed,
    Launched,
    Accepted,
    Processing,
    Retrying,
    AnswerReady,
    Failed,
    Done,
}

impl ReceiverLifecyclePhase {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Claimed => "claimed",
            Self::Launched => "launched",
            Self::Accepted => "accepted",
            Self::Processing => "processing",
            Self::Retrying => "retrying",
            Self::AnswerReady => "answer-ready",
            Self::Failed => "failed",
            Self::Done => "done",
        }
    }
}

/// Finite provider-delivery phase allowed in logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReceiverDeliveryPhase {
    Ready,
    Retrying,
    Acknowledged,
    Failed,
    Ambiguous,
}

impl ReceiverDeliveryPhase {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Retrying => "retrying",
            Self::Acknowledged => "acknowledged",
            Self::Failed => "failed",
            Self::Ambiguous => "ambiguous",
        }
    }
}

/// Stable content-free transition reason allowed in logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReceiverLifecycleReason {
    AcceptedStall,
    PreAcceptanceTimeout,
    PreAcceptanceExhausted,
    AbsoluteWorkExpired,
    RecoveryExpired,
    RecoveryExhausted,
    RecoveryPlanningFailed,
    RecoveryRegistrationFailed,
    RecoverySpawnFailed,
    RecoveryShutdown,
    NativeSessionUnavailable,
    IncompleteLegacyCompletion,
    NoticeNoAuthorizedDestination,
    TransportUnavailable,
    ProviderAcknowledged,
    Authorization,
    Credentials,
    InvalidRequest,
    ProviderRejected,
    RetryExhausted,
    IdempotencyWindowExpired,
    ProviderAcceptanceUnknown,
    ProviderAcknowledgementMalformed,
    ResultCommitUnknown,
}

impl ReceiverLifecycleReason {
    const fn as_str(self) -> &'static str {
        match self {
            Self::AcceptedStall => "accepted-stall",
            Self::PreAcceptanceTimeout => "pre-acceptance-timeout",
            Self::PreAcceptanceExhausted => "pre-acceptance-exhausted",
            Self::AbsoluteWorkExpired => "absolute-work-expired",
            Self::RecoveryExpired => "recovery-expired",
            Self::RecoveryExhausted => "recovery-exhausted",
            Self::RecoveryPlanningFailed => "recovery-planning-failed",
            Self::RecoveryRegistrationFailed => "recovery-registration-failed",
            Self::RecoverySpawnFailed => "recovery-spawn-failed",
            Self::RecoveryShutdown => "recovery-shutdown",
            Self::NativeSessionUnavailable => "native-session-unavailable",
            Self::IncompleteLegacyCompletion => "incomplete-legacy-completion",
            Self::NoticeNoAuthorizedDestination => "notice-no-authorized-destination",
            Self::TransportUnavailable => "transport-unavailable",
            Self::ProviderAcknowledged => "provider-acknowledged",
            Self::Authorization => "authorization",
            Self::Credentials => "credentials",
            Self::InvalidRequest => "invalid-request",
            Self::ProviderRejected => "provider-rejected",
            Self::RetryExhausted => "retry-exhausted",
            Self::IdempotencyWindowExpired => "idempotency-window-expired",
            Self::ProviderAcceptanceUnknown => "provider-acceptance-unknown",
            Self::ProviderAcknowledgementMalformed => "provider-acknowledgement-malformed",
            Self::ResultCommitUnknown => "result-commit-unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EventName {
    Ingress,
    Claim,
    Launch,
    Acceptance,
    Progress,
    Recovery,
    AnswerReadiness,
    CleanupPromotion,
    DeliveryResult,
    TerminalAdvancement,
}

impl EventName {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Ingress => "ingress",
            Self::Claim => "claim",
            Self::Launch => "launch",
            Self::Acceptance => "acceptance",
            Self::Progress => "progress",
            Self::Recovery => "recovery",
            Self::AnswerReadiness => "answer-readiness",
            Self::CleanupPromotion => "cleanup-promotion",
            Self::DeliveryResult => "delivery-result",
            Self::TerminalAdvancement => "terminal-advancement",
        }
    }
}

/// One typed lifecycle log record with no identity or content-bearing fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReceiverLifecycleEvent {
    name: EventName,
    phase: Option<ReceiverLifecyclePhase>,
    delivery_phase: Option<ReceiverDeliveryPhase>,
    queue_depth: Option<usize>,
    recovery: Option<(u32, u32)>,
    cleanup_gated: Option<usize>,
    reason: Option<ReceiverLifecycleReason>,
}

impl ReceiverLifecycleEvent {
    pub(crate) const fn ingress(queue_depth: usize) -> Self {
        Self::new(EventName::Ingress)
            .with_phase(ReceiverLifecyclePhase::Queued)
            .with_queue_depth(queue_depth)
    }

    pub(crate) const fn claim(queue_depth: usize) -> Self {
        Self::new(EventName::Claim)
            .with_phase(ReceiverLifecyclePhase::Claimed)
            .with_queue_depth(queue_depth)
    }

    pub(crate) const fn launch(recovery_ordinal: u32, recovery_limit: u32) -> Self {
        Self::new(EventName::Launch)
            .with_phase(ReceiverLifecyclePhase::Launched)
            .with_recovery(recovery_ordinal, recovery_limit)
    }

    pub(crate) const fn observation(phase: ReceiverLifecyclePhase) -> Self {
        let name = match phase {
            ReceiverLifecyclePhase::Accepted => EventName::Acceptance,
            _ => EventName::Progress,
        };
        Self::new(name).with_phase(phase)
    }

    pub(crate) const fn recovery(
        phase: ReceiverLifecyclePhase,
        ordinal: u32,
        limit: u32,
        reason: ReceiverLifecycleReason,
    ) -> Self {
        Self::new(EventName::Recovery)
            .with_phase(phase)
            .with_recovery(ordinal, limit)
            .with_reason(reason)
    }

    pub(crate) const fn answer_ready(cleanup_gated: usize) -> Self {
        Self::new(EventName::AnswerReadiness)
            .with_phase(ReceiverLifecyclePhase::AnswerReady)
            .with_cleanup_gated(cleanup_gated)
    }

    pub(crate) const fn cleanup_promotion(cleanup_gated: usize) -> Self {
        Self::new(EventName::CleanupPromotion)
            .with_delivery_phase(ReceiverDeliveryPhase::Ready)
            .with_cleanup_gated(cleanup_gated)
    }

    pub(crate) const fn delivery_result(
        phase: ReceiverDeliveryPhase,
        reason: ReceiverLifecycleReason,
    ) -> Self {
        Self::new(EventName::DeliveryResult)
            .with_delivery_phase(phase)
            .with_reason(reason)
    }

    pub(crate) const fn terminal(
        phase: ReceiverLifecyclePhase,
        queue_depth: usize,
        reason: ReceiverLifecycleReason,
    ) -> Self {
        Self::new(EventName::TerminalAdvancement)
            .with_phase(phase)
            .with_queue_depth(queue_depth)
            .with_reason(reason)
    }

    const fn new(name: EventName) -> Self {
        Self {
            name,
            phase: None,
            delivery_phase: None,
            queue_depth: None,
            recovery: None,
            cleanup_gated: None,
            reason: None,
        }
    }

    const fn with_phase(mut self, phase: ReceiverLifecyclePhase) -> Self {
        self.phase = Some(phase);
        self
    }

    const fn with_delivery_phase(mut self, phase: ReceiverDeliveryPhase) -> Self {
        self.delivery_phase = Some(phase);
        self
    }

    const fn with_queue_depth(mut self, depth: usize) -> Self {
        self.queue_depth = Some(depth);
        self
    }

    const fn with_recovery(mut self, ordinal: u32, limit: u32) -> Self {
        self.recovery = Some((ordinal, limit));
        self
    }

    const fn with_cleanup_gated(mut self, count: usize) -> Self {
        self.cleanup_gated = Some(count);
        self
    }

    const fn with_reason(mut self, reason: ReceiverLifecycleReason) -> Self {
        self.reason = Some(reason);
        self
    }
}

impl Display for ReceiverLifecycleEvent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut line = format!("receiver lifecycle event={}", self.name.as_str());
        if let Some(phase) = self.phase {
            write!(line, " phase={}", phase.as_str())?;
        }
        if let Some(phase) = self.delivery_phase {
            write!(line, " delivery_phase={}", phase.as_str())?;
        }
        if let Some(depth) = self.queue_depth {
            write!(line, " queue_depth={depth}")?;
        }
        if let Some((ordinal, limit)) = self.recovery {
            write!(line, " recovery={ordinal}/{limit}")?;
        }
        if let Some(count) = self.cleanup_gated {
            write!(line, " cleanup_gated={count}")?;
        }
        if let Some(reason) = self.reason {
            write!(line, " reason={}", reason.as_str())?;
        }
        formatter.write_str(&line)
    }
}
