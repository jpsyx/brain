use std::error::Error;
use std::fmt::{Display, Formatter};

use uuid::Uuid;

use super::{ReceiverConversationIdentity, ReceiverJobState};

/// Maximum pre-acceptance process-launch attempts for one durable job.
pub const MAX_RECEIVER_LAUNCH_ATTEMPTS: u32 = 3;

/// Stable, content-free reason one receiver process failed before acceptance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverLaunchFailure {
    Planning,
    Registration,
    Allocation,
    Spawn,
}

impl ReceiverLaunchFailure {
    pub const ALL: [Self; 4] = [
        Self::Planning,
        Self::Registration,
        Self::Allocation,
        Self::Spawn,
    ];

    pub(super) const fn expected_state(self) -> ReceiverJobState {
        match self {
            Self::Planning | Self::Registration => ReceiverJobState::Claimed,
            Self::Allocation | Self::Spawn => ReceiverJobState::Launching,
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Planning => "launch-planning",
            Self::Registration => "launch-registration",
            Self::Allocation => "launch-allocation",
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
    conversation_id: ReceiverConversationId,
    inbound: crate::server::receiver::InboundJob,
    state: ReceiverJobState,
    retry_count: u32,
    retry_at_unix_ms: Option<u64>,
    retry_from_state: Option<ReceiverJobState>,
    last_error: Option<String>,
}

pub(super) struct ReceiverRetryMetadata {
    pub(super) count: u32,
    pub(super) at_unix_ms: Option<u64>,
    pub(super) from_state: Option<ReceiverJobState>,
    pub(super) last_error: Option<String>,
}

impl ReceiverJob {
    pub(super) fn from_stored(
        id: ReceiverJobId,
        conversation_id: ReceiverConversationId,
        inbound: crate::server::receiver::InboundJob,
        state: ReceiverJobState,
        retry: ReceiverRetryMetadata,
    ) -> Self {
        Self {
            id,
            conversation_id,
            inbound,
            state,
            retry_count: retry.count,
            retry_at_unix_ms: retry.at_unix_ms,
            retry_from_state: retry.from_state,
            last_error: retry.last_error,
        }
    }

    #[must_use]
    pub const fn id(&self) -> ReceiverJobId {
        self.id
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
