use std::error::Error;
use std::fmt::{Display, Formatter};

use uuid::Uuid;

use super::{
    ReceiverAttemptKind, ReceiverConversationIdentity, ReceiverJobState, ReceiverRecoverySnapshot,
};

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

    pub(super) const fn expected_state(self) -> ReceiverJobState {
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

/// Exhaustive durable result of establishing exact cleanup for a spawned recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiverRecoveryCleanupOutcome {
    Exact(ReceiverReconciliationEffect),
    Changed,
}

/// One finite writer lease for handing a terminal unavailable notice to the
/// process-local delivery worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiverUnavailableNoticeClaim {
    job_id: ReceiverJobId,
    token: ReceiverJobToken,
    owner: String,
    expires_at_unix_ms: u64,
    inbound: crate::server::receiver::InboundJob,
}

impl ReceiverUnavailableNoticeClaim {
    pub(super) fn new(job: &ReceiverJob, owner: String, expires_at_unix_ms: u64) -> Self {
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

impl ReceiverRecoveryFailure {
    pub(super) const fn reason(self) -> ReceiverReconciliationReason {
        match self {
            Self::Planning => ReceiverReconciliationReason::RecoveryPlanningFailed,
            Self::Registration => ReceiverReconciliationReason::RecoveryRegistrationFailed,
            Self::Spawn => ReceiverReconciliationReason::RecoverySpawnFailed,
            Self::Shutdown => ReceiverReconciliationReason::RecoveryShutdown,
        }
    }
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
    pub(super) const fn as_str(self) -> &'static str {
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

    pub(super) fn parse(value: &str) -> Option<Self> {
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiverReconciliationEffect {
    action: ReceiverReconciliationAction,
    reason: ReceiverReconciliationReason,
    job_id: ReceiverJobId,
    token: ReceiverJobToken,
    cleanup_instance: Option<String>,
    cleanup_session_id: Option<String>,
}

impl ReceiverReconciliationEffect {
    pub(super) fn new(
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

/// One frontend-neutral nonterminal receiver lifecycle fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverNonterminalObservationPhase {
    Accepted,
    Progressing,
}

/// Content-free evidence and authorization timing for one post-spawn launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiverLaunchObservation {
    pub token: ReceiverJobToken,
    pub instance: String,
    pub session_id: String,
    pub observed_at_unix_ms: u64,
    pub authorized_at_unix_ms: u64,
}

/// Content-free evidence and authorization timing for one nonterminal lifecycle fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiverObservation {
    pub token: ReceiverJobToken,
    pub instance: String,
    pub session_id: String,
    pub phase: ReceiverNonterminalObservationPhase,
    pub revision: u64,
    pub observed_at_unix_ms: u64,
    pub authorized_at_unix_ms: u64,
}

/// Every newly represented lifecycle boundary from one normalized snapshot.
#[derive(Clone, PartialEq, Eq)]
pub struct ReceiverObservationSet {
    pub token: ReceiverJobToken,
    pub instance: String,
    pub session_id: String,
    pub revision: u64,
    pub accepted_at_unix_ms: Option<u64>,
    pub progressing_at_unix_ms: Option<u64>,
    pub latest_progress_at_unix_ms: Option<u64>,
    pub completed_at_unix_ms: Option<u64>,
    pub authorized_at_unix_ms: u64,
}

impl std::fmt::Debug for ReceiverObservationSet {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiverObservationSet(<redacted>)")
    }
}

impl ReceiverObservationSet {
    pub(crate) fn from_agent_observation(
        token: ReceiverJobToken,
        registration: &ReceiverSessionAttribution,
        result: &crate::agent::AgentObservationResult,
        authorized_at_unix_ms: u64,
    ) -> Self {
        let mut accepted_at_unix_ms = None;
        let mut progressing_at_unix_ms = None;
        let mut completed_at_unix_ms = None;
        for boundary in result.boundaries() {
            match boundary.phase() {
                crate::agent::AgentObservationPhase::Launched => {}
                crate::agent::AgentObservationPhase::Accepted => {
                    accepted_at_unix_ms = Some(boundary.observed_at_unix_ms());
                }
                crate::agent::AgentObservationPhase::Progressing => {
                    progressing_at_unix_ms = Some(boundary.observed_at_unix_ms());
                }
                crate::agent::AgentObservationPhase::Completed => {
                    completed_at_unix_ms = Some(boundary.observed_at_unix_ms());
                }
            }
        }
        Self {
            token,
            instance: registration.instance().to_owned(),
            session_id: result.session().as_str().to_owned(),
            revision: result.next_cursor().durable_revision(),
            accepted_at_unix_ms,
            progressing_at_unix_ms,
            latest_progress_at_unix_ms: result
                .progress_pulse()
                .map(crate::agent::AgentProgressPulse::observed_at_unix_ms),
            completed_at_unix_ms,
            authorized_at_unix_ms,
        }
    }
}

/// Immutable identifier for one workspace-scoped receiver job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReceiverJobId(Uuid);

impl ReceiverJobId {
    pub(super) fn parse(value: &str) -> anyhow::Result<Self> {
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
    pub(super) fn new() -> Self {
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
    pub(super) fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub(super) fn parse(value: &str) -> anyhow::Result<Self> {
        Ok(Self(Uuid::parse_str(value)?))
    }
}

impl Display for ReceiverConversationId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Exact durable attribution registered before one isolated receiver launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiverSessionAttribution {
    conversation_id: ReceiverConversationId,
    instance: String,
    registered_session: crate::agent::AgentSession,
    scope: crate::agent::SessionScope,
}

impl ReceiverSessionAttribution {
    pub(super) fn new(
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

/// Exact durable identity and timings required to complete one receiver job.
#[derive(Debug, Clone, Copy)]
pub struct ReceiverCompletionRequest<'a> {
    pub job_id: ReceiverJobId,
    pub token: ReceiverJobToken,
    pub owner: &'a str,
    pub registration: &'a ReceiverSessionAttribution,
    pub completed_session: &'a crate::agent::AgentSession,
    pub observed_at_unix_ms: u64,
    pub authorized_at_unix_ms: u64,
}

/// Current frontend-owned native session attached to a logical conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiverSessionBinding {
    frontend: crate::agent::AgentKind,
    native_session_id: String,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiverSessionPlan {
    ResumeNative(String),
    FreshFromTranscript(String),
}

/// One persisted logical receiver conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiverConversation {
    id: ReceiverConversationId,
    identity: ReceiverConversationIdentity,
    transcript_markdown: String,
    binding: Option<ReceiverSessionBinding>,
}

impl ReceiverConversation {
    pub(super) fn from_stored(
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

/// One persisted receiver job and its retry metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiverJob {
    id: ReceiverJobId,
    token: ReceiverJobToken,
    conversation_id: ReceiverConversationId,
    inbound: crate::server::receiver::InboundJob,
    state: ReceiverJobState,
    retry_count: u32,
    retry_at_unix_ms: Option<u64>,
    retry_from_state: Option<ReceiverJobState>,
    last_error: Option<String>,
    launched_at_unix_ms: Option<u64>,
    accepted_at_unix_ms: Option<u64>,
    progressing_at_unix_ms: Option<u64>,
    completed_at_unix_ms: Option<u64>,
    observation_instance: Option<String>,
    observation_session_id: Option<String>,
    observation_revision: u64,
    attempt_accepted_at_unix_ms: Option<u64>,
    attempt_progressing_at_unix_ms: Option<u64>,
    latest_progress_at_unix_ms: Option<u64>,
    launch_expires_at_unix_ms: Option<u64>,
    acceptance_expires_at_unix_ms: Option<u64>,
    progress_expires_at_unix_ms: Option<u64>,
    recovery_expires_at_unix_ms: Option<u64>,
    absolute_work_expires_at_unix_ms: Option<u64>,
    recovery_count: u32,
    attempt_kind: ReceiverAttemptKind,
    pending_unavailable_notice: bool,
    recovery_cleanup_instance: Option<String>,
    recovery_cleanup_session_id: Option<String>,
}

pub(super) struct ReceiverRetryMetadata {
    pub(super) count: u32,
    pub(super) at_unix_ms: Option<u64>,
    pub(super) from_state: Option<ReceiverJobState>,
    pub(super) last_error: Option<String>,
}

pub(super) struct ReceiverStoredMetadata {
    pub(super) state: ReceiverJobState,
    pub(super) retry: ReceiverRetryMetadata,
    pub(super) observation: ReceiverObservationMetadata,
    pub(super) recovery: ReceiverRecoveryMetadata,
}

impl ReceiverJob {
    pub(super) fn from_stored(
        id: ReceiverJobId,
        token: ReceiverJobToken,
        conversation_id: ReceiverConversationId,
        inbound: crate::server::receiver::InboundJob,
        metadata: ReceiverStoredMetadata,
    ) -> Self {
        let ReceiverStoredMetadata {
            state,
            retry,
            observation,
            recovery,
        } = metadata;
        Self {
            id,
            token,
            conversation_id,
            inbound,
            state,
            retry_count: retry.count,
            retry_at_unix_ms: retry.at_unix_ms,
            retry_from_state: retry.from_state,
            last_error: retry.last_error,
            launched_at_unix_ms: observation.launched_at_unix_ms,
            accepted_at_unix_ms: observation.accepted_at_unix_ms,
            progressing_at_unix_ms: observation.progressing_at_unix_ms,
            completed_at_unix_ms: observation.completed_at_unix_ms,
            observation_instance: observation.instance,
            observation_session_id: observation.session_id,
            observation_revision: observation.revision,
            attempt_accepted_at_unix_ms: observation.attempt_accepted_at_unix_ms,
            attempt_progressing_at_unix_ms: observation.attempt_progressing_at_unix_ms,
            latest_progress_at_unix_ms: recovery.latest_progress_at_unix_ms,
            launch_expires_at_unix_ms: recovery.launch_expires_at_unix_ms,
            acceptance_expires_at_unix_ms: recovery.acceptance_expires_at_unix_ms,
            progress_expires_at_unix_ms: recovery.progress_expires_at_unix_ms,
            recovery_expires_at_unix_ms: recovery.recovery_expires_at_unix_ms,
            absolute_work_expires_at_unix_ms: recovery.absolute_work_expires_at_unix_ms,
            recovery_count: recovery.recovery_count,
            attempt_kind: recovery.attempt_kind,
            pending_unavailable_notice: recovery.pending_unavailable_notice,
            recovery_cleanup_instance: recovery.cleanup_instance,
            recovery_cleanup_session_id: recovery.cleanup_session_id,
        }
    }

    #[must_use]
    pub const fn id(&self) -> ReceiverJobId {
        self.id
    }

    #[must_use]
    pub const fn token(&self) -> ReceiverJobToken {
        self.token
    }

    #[must_use]
    pub const fn conversation_id(&self) -> ReceiverConversationId {
        self.conversation_id
    }

    #[must_use]
    pub const fn inbound(&self) -> &crate::server::receiver::InboundJob {
        &self.inbound
    }

    #[must_use]
    pub const fn state(&self) -> ReceiverJobState {
        self.state
    }

    #[must_use]
    pub const fn retry_count(&self) -> u32 {
        self.retry_count
    }

    #[must_use]
    pub const fn retry_at_unix_ms(&self) -> Option<u64> {
        self.retry_at_unix_ms
    }

    #[must_use]
    pub const fn retry_from_state(&self) -> Option<ReceiverJobState> {
        self.retry_from_state
    }

    #[must_use]
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    #[must_use]
    pub const fn launched_at_unix_ms(&self) -> Option<u64> {
        self.launched_at_unix_ms
    }
    #[must_use]
    pub const fn accepted_at_unix_ms(&self) -> Option<u64> {
        self.accepted_at_unix_ms
    }
    #[must_use]
    pub const fn progressing_at_unix_ms(&self) -> Option<u64> {
        self.progressing_at_unix_ms
    }
    #[must_use]
    pub const fn completed_at_unix_ms(&self) -> Option<u64> {
        self.completed_at_unix_ms
    }
    #[must_use]
    pub fn observation_instance(&self) -> Option<&str> {
        self.observation_instance.as_deref()
    }
    #[must_use]
    pub fn observation_session_id(&self) -> Option<&str> {
        self.observation_session_id.as_deref()
    }
    #[must_use]
    pub const fn observation_revision(&self) -> u64 {
        self.observation_revision
    }
    /// Rebuild the frontend-neutral cursor for only the current attempt.
    ///
    /// # Errors
    ///
    /// Returns an error when the persisted current-attempt evidence is not representable.
    pub fn observation_cursor(
        &self,
    ) -> Result<crate::agent::AgentObservationCursor, crate::agent::AgentObservationError> {
        let attempt_latest_progress = self
            .attempt_progressing_at_unix_ms
            .and(self.latest_progress_at_unix_ms);
        crate::agent::AgentObservationCursor::from_durable(
            self.observation_revision,
            self.attempt_accepted_at_unix_ms,
            self.attempt_progressing_at_unix_ms,
            attempt_latest_progress,
            self.completed_at_unix_ms,
        )
    }
    #[must_use]
    pub const fn attempt_accepted_at_unix_ms(&self) -> Option<u64> {
        self.attempt_accepted_at_unix_ms
    }
    #[must_use]
    pub const fn attempt_progressing_at_unix_ms(&self) -> Option<u64> {
        self.attempt_progressing_at_unix_ms
    }
    #[must_use]
    pub const fn latest_progress_at_unix_ms(&self) -> Option<u64> {
        self.latest_progress_at_unix_ms
    }
    #[must_use]
    pub const fn launch_expires_at_unix_ms(&self) -> Option<u64> {
        self.launch_expires_at_unix_ms
    }
    #[must_use]
    pub const fn acceptance_expires_at_unix_ms(&self) -> Option<u64> {
        self.acceptance_expires_at_unix_ms
    }
    #[must_use]
    pub const fn progress_expires_at_unix_ms(&self) -> Option<u64> {
        self.progress_expires_at_unix_ms
    }
    #[must_use]
    pub const fn recovery_expires_at_unix_ms(&self) -> Option<u64> {
        self.recovery_expires_at_unix_ms
    }
    #[must_use]
    pub const fn absolute_work_expires_at_unix_ms(&self) -> Option<u64> {
        self.absolute_work_expires_at_unix_ms
    }
    #[must_use]
    pub const fn recovery_count(&self) -> u32 {
        self.recovery_count
    }
    #[must_use]
    pub const fn attempt_kind(&self) -> ReceiverAttemptKind {
        self.attempt_kind
    }
    #[must_use]
    pub const fn pending_unavailable_notice(&self) -> bool {
        self.pending_unavailable_notice
    }
    #[must_use]
    pub fn recovery_cleanup_instance(&self) -> Option<&str> {
        self.recovery_cleanup_instance.as_deref()
    }
    #[must_use]
    pub fn recovery_cleanup_session_id(&self) -> Option<&str> {
        self.recovery_cleanup_session_id.as_deref()
    }
    #[must_use]
    pub const fn recovery_snapshot(&self, now_unix_ms: u64) -> ReceiverRecoverySnapshot {
        ReceiverRecoverySnapshot {
            state: self.state,
            attempt_kind: self.attempt_kind,
            launch_attempt_count: self.retry_count,
            recovery_count: self.recovery_count,
            now_unix_ms,
            launch_expires_at_unix_ms: self.launch_expires_at_unix_ms,
            acceptance_expires_at_unix_ms: self.acceptance_expires_at_unix_ms,
            progress_expires_at_unix_ms: self.progress_expires_at_unix_ms,
            recovery_expires_at_unix_ms: self.recovery_expires_at_unix_ms,
            absolute_work_expires_at_unix_ms: self.absolute_work_expires_at_unix_ms,
        }
    }
}

pub(super) struct ReceiverObservationMetadata {
    pub(super) launched_at_unix_ms: Option<u64>,
    pub(super) accepted_at_unix_ms: Option<u64>,
    pub(super) progressing_at_unix_ms: Option<u64>,
    pub(super) completed_at_unix_ms: Option<u64>,
    pub(super) instance: Option<String>,
    pub(super) session_id: Option<String>,
    pub(super) revision: u64,
    pub(super) attempt_accepted_at_unix_ms: Option<u64>,
    pub(super) attempt_progressing_at_unix_ms: Option<u64>,
}

pub(super) struct ReceiverRecoveryMetadata {
    pub(super) latest_progress_at_unix_ms: Option<u64>,
    pub(super) launch_expires_at_unix_ms: Option<u64>,
    pub(super) acceptance_expires_at_unix_ms: Option<u64>,
    pub(super) progress_expires_at_unix_ms: Option<u64>,
    pub(super) recovery_expires_at_unix_ms: Option<u64>,
    pub(super) absolute_work_expires_at_unix_ms: Option<u64>,
    pub(super) recovery_count: u32,
    pub(super) attempt_kind: ReceiverAttemptKind,
    pub(super) pending_unavailable_notice: bool,
    pub(super) cleanup_instance: Option<String>,
    pub(super) cleanup_session_id: Option<String>,
}

/// One live FIFO claim with the immutable job and logical conversation it owns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiverRunClaim {
    claim: ReceiverClaim,
    job: ReceiverJob,
    conversation: ReceiverConversation,
}

impl ReceiverRunClaim {
    pub(super) const fn new(
        claim: ReceiverClaim,
        job: ReceiverJob,
        conversation: ReceiverConversation,
    ) -> Self {
        Self {
            claim,
            job,
            conversation,
        }
    }

    #[must_use]
    pub const fn claim(&self) -> &ReceiverClaim {
        &self.claim
    }

    #[must_use]
    pub const fn job(&self) -> &ReceiverJob {
        &self.job
    }

    #[must_use]
    pub const fn conversation(&self) -> &ReceiverConversation {
        &self.conversation
    }
}

/// Result of durable receiver admission or provider retry deduplication.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReceiverAcceptance {
    job_id: ReceiverJobId,
    conversation_id: ReceiverConversationId,
    was_inserted: bool,
}

impl ReceiverAcceptance {
    pub(super) const fn new(
        job_id: ReceiverJobId,
        conversation_id: ReceiverConversationId,
        was_inserted: bool,
    ) -> Self {
        Self {
            job_id,
            conversation_id,
            was_inserted,
        }
    }

    #[must_use]
    pub const fn job_id(self) -> ReceiverJobId {
        self.job_id
    }

    #[must_use]
    pub const fn conversation_id(self) -> ReceiverConversationId {
        self.conversation_id
    }

    #[must_use]
    pub const fn was_inserted(self) -> bool {
        self.was_inserted
    }
}

/// Expiring non-destructive ownership of one receiver job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiverClaim {
    job_id: ReceiverJobId,
    owner: String,
    expires_at_unix_ms: u64,
}

impl ReceiverClaim {
    pub(super) fn new(job_id: ReceiverJobId, owner: String, expires_at_unix_ms: u64) -> Self {
        Self {
            job_id,
            owner,
            expires_at_unix_ms,
        }
    }

    #[must_use]
    pub const fn job_id(&self) -> ReceiverJobId {
        self.job_id
    }

    #[must_use]
    pub fn owner(&self) -> &str {
        &self.owner
    }

    #[must_use]
    pub const fn expires_at_unix_ms(&self) -> u64 {
        self.expires_at_unix_ms
    }
}
