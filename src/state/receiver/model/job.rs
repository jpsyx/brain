use super::{ReceiverConversationId, ReceiverJobId, ReceiverJobToken};
use crate::state::{ReceiverAttemptKind, ReceiverJobState, ReceiverRecoverySnapshot};

/// One persisted receiver job and its retry metadata.
#[derive(Clone, PartialEq, Eq)]
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

impl std::fmt::Debug for ReceiverJob {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiverJob(<redacted>)")
    }
}

pub(in crate::state::receiver) struct ReceiverRetryMetadata {
    pub(in crate::state::receiver) count: u32,
    pub(in crate::state::receiver) at_unix_ms: Option<u64>,
    pub(in crate::state::receiver) from_state: Option<ReceiverJobState>,
    pub(in crate::state::receiver) last_error: Option<String>,
}

pub(in crate::state::receiver) struct ReceiverStoredMetadata {
    pub(in crate::state::receiver) state: ReceiverJobState,
    pub(in crate::state::receiver) retry: ReceiverRetryMetadata,
    pub(in crate::state::receiver) observation: ReceiverObservationMetadata,
    pub(in crate::state::receiver) recovery: ReceiverRecoveryMetadata,
}

impl ReceiverJob {
    pub(in crate::state::receiver) fn from_stored(
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

pub(in crate::state::receiver) struct ReceiverObservationMetadata {
    pub(in crate::state::receiver) launched_at_unix_ms: Option<u64>,
    pub(in crate::state::receiver) accepted_at_unix_ms: Option<u64>,
    pub(in crate::state::receiver) progressing_at_unix_ms: Option<u64>,
    pub(in crate::state::receiver) completed_at_unix_ms: Option<u64>,
    pub(in crate::state::receiver) instance: Option<String>,
    pub(in crate::state::receiver) session_id: Option<String>,
    pub(in crate::state::receiver) revision: u64,
    pub(in crate::state::receiver) attempt_accepted_at_unix_ms: Option<u64>,
    pub(in crate::state::receiver) attempt_progressing_at_unix_ms: Option<u64>,
}

pub(in crate::state::receiver) struct ReceiverRecoveryMetadata {
    pub(in crate::state::receiver) latest_progress_at_unix_ms: Option<u64>,
    pub(in crate::state::receiver) launch_expires_at_unix_ms: Option<u64>,
    pub(in crate::state::receiver) acceptance_expires_at_unix_ms: Option<u64>,
    pub(in crate::state::receiver) progress_expires_at_unix_ms: Option<u64>,
    pub(in crate::state::receiver) recovery_expires_at_unix_ms: Option<u64>,
    pub(in crate::state::receiver) absolute_work_expires_at_unix_ms: Option<u64>,
    pub(in crate::state::receiver) recovery_count: u32,
    pub(in crate::state::receiver) attempt_kind: ReceiverAttemptKind,
    pub(in crate::state::receiver) pending_unavailable_notice: bool,
    pub(in crate::state::receiver) cleanup_instance: Option<String>,
    pub(in crate::state::receiver) cleanup_session_id: Option<String>,
}
