use super::{MAX_RECEIVER_LAUNCH_ATTEMPTS, ReceiverJobState};

const RECEIVER_LAUNCH_LEASE_MS: u64 = 120_000;
const RECEIVER_ACCEPTANCE_LEASE_MS: u64 = 90_000;
const RECEIVER_PROGRESS_LEASE_MS: u64 = 300_000;
const RECEIVER_ABSOLUTE_WORK_LEASE_MS: u64 = 1_800_000;

/// Maximum same-session recovery launches after exact receiver acceptance.
pub const MAX_RECEIVER_RECOVERY_ATTEMPTS: u32 = 1;

/// Whether the current durable run is ordinary work or same-session recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverAttemptKind {
    Ordinary,
    Recovery,
}

impl ReceiverAttemptKind {
    pub(super) fn parse(value: &str) -> Option<Self> {
        match value {
            "ordinary" => Some(Self::Ordinary),
            "recovery" => Some(Self::Recovery),
            _ => None,
        }
    }
}

/// Persisted facts needed for one clock-injected recovery decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReceiverRecoverySnapshot {
    pub state: ReceiverJobState,
    pub attempt_kind: ReceiverAttemptKind,
    pub launch_attempt_count: u32,
    pub recovery_count: u32,
    pub now_unix_ms: u64,
    pub launch_expires_at_unix_ms: Option<u64>,
    pub acceptance_expires_at_unix_ms: Option<u64>,
    pub progress_expires_at_unix_ms: Option<u64>,
    pub recovery_expires_at_unix_ms: Option<u64>,
    pub absolute_work_expires_at_unix_ms: Option<u64>,
}

/// Semantic outcome for one durable receiver recovery snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverRecoveryDecision {
    Wait,
    RequeuePreAcceptance,
    RecoverSameSession,
    TerminalFailure,
    IncompleteLegacyCompletion,
}

/// Decide the next recovery action without reading a clock or mutating state.
#[must_use]
pub fn decide_receiver_recovery(snapshot: ReceiverRecoverySnapshot) -> ReceiverRecoveryDecision {
    match snapshot.state {
        ReceiverJobState::Queued
        | ReceiverJobState::Retrying
        | ReceiverJobState::Failed
        | ReceiverJobState::Done => ReceiverRecoveryDecision::Wait,
        ReceiverJobState::AnswerReady | ReceiverJobState::Delivering => {
            ReceiverRecoveryDecision::IncompleteLegacyCompletion
        }
        ReceiverJobState::Claimed | ReceiverJobState::Launching => {
            decide_pre_acceptance(snapshot, snapshot.launch_expires_at_unix_ms)
        }
        ReceiverJobState::Launched => {
            decide_pre_acceptance(snapshot, snapshot.acceptance_expires_at_unix_ms)
        }
        ReceiverJobState::Accepted | ReceiverJobState::Processing => decide_accepted(snapshot),
    }
}

fn decide_pre_acceptance(
    snapshot: ReceiverRecoverySnapshot,
    phase_expiry: Option<u64>,
) -> ReceiverRecoveryDecision {
    if !is_expired(snapshot.now_unix_ms, snapshot.recovery_expires_at_unix_ms)
        && !is_expired(snapshot.now_unix_ms, phase_expiry)
    {
        return ReceiverRecoveryDecision::Wait;
    }
    if matches!(snapshot.attempt_kind, ReceiverAttemptKind::Recovery)
        || snapshot.launch_attempt_count >= MAX_RECEIVER_LAUNCH_ATTEMPTS - 1
    {
        ReceiverRecoveryDecision::TerminalFailure
    } else {
        ReceiverRecoveryDecision::RequeuePreAcceptance
    }
}

fn decide_accepted(snapshot: ReceiverRecoverySnapshot) -> ReceiverRecoveryDecision {
    if is_expired(
        snapshot.now_unix_ms,
        snapshot.absolute_work_expires_at_unix_ms,
    ) || is_expired(snapshot.now_unix_ms, snapshot.recovery_expires_at_unix_ms)
    {
        return ReceiverRecoveryDecision::TerminalFailure;
    }
    if !is_expired(snapshot.now_unix_ms, snapshot.progress_expires_at_unix_ms) {
        return ReceiverRecoveryDecision::Wait;
    }
    if matches!(snapshot.attempt_kind, ReceiverAttemptKind::Ordinary)
        && snapshot.recovery_count < MAX_RECEIVER_RECOVERY_ATTEMPTS
    {
        ReceiverRecoveryDecision::RecoverSameSession
    } else {
        ReceiverRecoveryDecision::TerminalFailure
    }
}

fn is_expired(now_unix_ms: u64, expires_at_unix_ms: Option<u64>) -> bool {
    expires_at_unix_ms.is_some_and(|expires_at| now_unix_ms >= expires_at)
}

/// Recovery deadlines established only after exact acceptance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReceiverLifecycleDeadlines {
    pub progress_expires_at_unix_ms: u64,
    pub absolute_work_expires_at_unix_ms: u64,
    pub latest_progress_at_unix_ms: Option<u64>,
}

impl ReceiverLifecycleDeadlines {
    /// Establish progress and immutable absolute limits from authorization time.
    #[must_use]
    pub fn after_acceptance(authorized_at_unix_ms: u64, _accepted_at_unix_ms: u64) -> Self {
        let absolute_work_expires_at_unix_ms =
            authorized_at_unix_ms.saturating_add(RECEIVER_ABSOLUTE_WORK_LEASE_MS);
        Self {
            progress_expires_at_unix_ms: authorized_at_unix_ms
                .saturating_add(RECEIVER_PROGRESS_LEASE_MS)
                .min(absolute_work_expires_at_unix_ms),
            absolute_work_expires_at_unix_ms,
            latest_progress_at_unix_ms: None,
        }
    }

    /// Renew progress from authorization time while retaining the absolute limit.
    #[must_use]
    pub fn after_progress(
        self,
        authorized_at_unix_ms: u64,
        latest_progress_at_unix_ms: u64,
    ) -> Self {
        Self {
            progress_expires_at_unix_ms: authorized_at_unix_ms
                .saturating_add(RECEIVER_PROGRESS_LEASE_MS)
                .min(self.absolute_work_expires_at_unix_ms),
            absolute_work_expires_at_unix_ms: self.absolute_work_expires_at_unix_ms,
            latest_progress_at_unix_ms: Some(latest_progress_at_unix_ms),
        }
    }
}

/// Establish the pre-spawn deadline from a trusted local clock observation.
#[must_use]
pub const fn receiver_launch_expires_at(authorized_at_unix_ms: u64) -> u64 {
    authorized_at_unix_ms.saturating_add(RECEIVER_LAUNCH_LEASE_MS)
}

/// Establish the post-spawn acceptance deadline from trusted authorization time.
#[must_use]
pub const fn receiver_acceptance_expires_at(authorized_at_unix_ms: u64) -> u64 {
    authorized_at_unix_ms.saturating_add(RECEIVER_ACCEPTANCE_LEASE_MS)
}
