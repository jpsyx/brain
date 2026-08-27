use super::{ReceiverJob, ReceiverJobId, ReceiverJobToken};
use crate::state::ReceiverJobState;

/// Maximum pre-acceptance process-launch attempts for one durable job.
pub const MAX_RECEIVER_LAUNCH_ATTEMPTS: u32 = 3;

/// Stable, content-free reason one receiver process failed before acceptance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverLaunchFailure {
    Planning,
    Registration,
    Spawn,
}

impl ReceiverLaunchFailure {
    pub const ALL: [Self; 3] = [Self::Planning, Self::Registration, Self::Spawn];

    pub(in crate::state::receiver) const fn expected_state(self) -> ReceiverJobState {
        match self {
            Self::Planning | Self::Registration => ReceiverJobState::Claimed,
            Self::Spawn => ReceiverJobState::Launching,
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Planning => "launch-planning",
            Self::Registration => "launch-registration",
            Self::Spawn => "launch-spawn",
        }
    }
}

/// Durable result of recording one pre-acceptance launch failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverLaunchRetryOutcome {
    Scheduled,
    Exhausted,
}

/// Content-free category for one claimed recovery attempt that could not launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverRecoveryFailure {
    Planning,
    Registration,
    Spawn,
    Shutdown,
}

impl ReceiverRecoveryFailure {
    pub(in crate::state::receiver) const fn reason(self) -> ReceiverReconciliationReason {
        match self {
            Self::Planning => ReceiverReconciliationReason::RecoveryPlanningFailed,
            Self::Registration => ReceiverReconciliationReason::RecoveryRegistrationFailed,
            Self::Spawn => ReceiverReconciliationReason::RecoverySpawnFailed,
            Self::Shutdown => ReceiverReconciliationReason::RecoveryShutdown,
        }
    }
}

/// Exhaustive durable result of establishing exact cleanup for a spawned recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiverRecoveryCleanupOutcome {
    Exact(ReceiverReconciliationEffect),
    Changed,
}

/// Durable transition published by one receiver reconciliation transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverReconciliationAction {
    RequeuePreAcceptance,
    ScheduleRecovery,
    TerminalFailure,
}

/// Stable content-free reason for one durable reconciliation transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverReconciliationReason {
    PreAcceptanceTimeout,
    PreAcceptanceExhausted,
    AcceptedStall,
    AbsoluteWorkExpired,
    RecoveryExpired,
    RecoveryExhausted,
    RecoveryPlanningFailed,
    RecoveryRegistrationFailed,
    RecoverySpawnFailed,
    RecoveryShutdown,
    NativeSessionUnavailable,
    IncompleteLegacyCompletion,
}

impl ReceiverReconciliationReason {
    pub(in crate::state::receiver) const fn as_str(self) -> &'static str {
        match self {
            Self::PreAcceptanceTimeout => "recovery-pre-acceptance-timeout",
            Self::PreAcceptanceExhausted => "recovery-pre-acceptance-exhausted",
            Self::AcceptedStall => "recovery-accepted-stall",
            Self::AbsoluteWorkExpired => "recovery-absolute-work-expired",
            Self::RecoveryExpired => "recovery-attempt-expired",
            Self::RecoveryExhausted => "recovery-attempt-exhausted",
            Self::RecoveryPlanningFailed => "recovery-launch-planning-failed",
            Self::RecoveryRegistrationFailed => "recovery-launch-registration-failed",
            Self::RecoverySpawnFailed => "recovery-launch-spawn-failed",
            Self::RecoveryShutdown => "recovery-launch-shutdown",
            Self::NativeSessionUnavailable => "recovery-native-session-unavailable",
            Self::IncompleteLegacyCompletion => "recovery-incomplete-legacy-completion",
        }
    }

    pub(in crate::state::receiver) fn parse(value: &str) -> Option<Self> {
        [
            Self::PreAcceptanceTimeout,
            Self::PreAcceptanceExhausted,
            Self::AcceptedStall,
            Self::AbsoluteWorkExpired,
            Self::RecoveryExpired,
            Self::RecoveryExhausted,
            Self::RecoveryPlanningFailed,
            Self::RecoveryRegistrationFailed,
            Self::RecoverySpawnFailed,
            Self::RecoveryShutdown,
            Self::NativeSessionUnavailable,
            Self::IncompleteLegacyCompletion,
        ]
        .into_iter()
        .find(|reason| reason.as_str() == value)
    }
}

/// Content-free identifiers for the one effect a reconciliation winner may execute.
#[derive(Clone, PartialEq, Eq)]
pub struct ReceiverReconciliationEffect {
    action: ReceiverReconciliationAction,
    reason: ReceiverReconciliationReason,
    job_id: ReceiverJobId,
    token: ReceiverJobToken,
    cleanup_instance: Option<String>,
    cleanup_session_id: Option<String>,
}

impl std::fmt::Debug for ReceiverReconciliationEffect {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiverReconciliationEffect(<redacted>)")
    }
}

impl ReceiverReconciliationEffect {
    pub(in crate::state::receiver) fn new(
        action: ReceiverReconciliationAction,
        reason: ReceiverReconciliationReason,
        job_id: ReceiverJobId,
        token: ReceiverJobToken,
        cleanup_instance: Option<String>,
        cleanup_session_id: Option<String>,
    ) -> Self {
        Self {
            action,
            reason,
            job_id,
            token,
            cleanup_instance,
            cleanup_session_id,
        }
    }

    #[must_use]
    pub const fn action(&self) -> ReceiverReconciliationAction {
        self.action
    }
    #[must_use]
    pub const fn reason(&self) -> ReceiverReconciliationReason {
        self.reason
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
    pub fn cleanup_instance(&self) -> Option<&str> {
        self.cleanup_instance.as_deref()
    }
    #[must_use]
    pub fn cleanup_session_id(&self) -> Option<&str> {
        self.cleanup_session_id.as_deref()
    }
}

/// One finite writer lease for handing a terminal unavailable notice to the
/// process-local delivery worker.
#[derive(Clone, PartialEq, Eq)]
pub struct ReceiverUnavailableNoticeClaim {
    job_id: ReceiverJobId,
    token: ReceiverJobToken,
    owner: String,
    expires_at_unix_ms: u64,
    inbound: crate::server::receiver::InboundJob,
}

impl std::fmt::Debug for ReceiverUnavailableNoticeClaim {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiverUnavailableNoticeClaim(<redacted>)")
    }
}

impl ReceiverUnavailableNoticeClaim {
    pub(in crate::state::receiver) fn new(
        job: &ReceiverJob,
        owner: String,
        expires_at_unix_ms: u64,
    ) -> Self {
        Self {
            job_id: job.id(),
            token: job.token(),
            owner,
            expires_at_unix_ms,
            inbound: job.inbound().clone(),
        }
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
    pub const fn inbound(&self) -> &crate::server::receiver::InboundJob {
        &self.inbound
    }
}
