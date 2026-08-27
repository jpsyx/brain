use super::{ReceiverConversation, ReceiverConversationId, ReceiverJob, ReceiverJobId};

/// One live FIFO claim with the immutable job and logical conversation it owns.
#[derive(Clone, PartialEq, Eq)]
pub struct ReceiverRunClaim {
    claim: ReceiverClaim,
    job: ReceiverJob,
    conversation: ReceiverConversation,
}

impl std::fmt::Debug for ReceiverRunClaim {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiverRunClaim(<redacted>)")
    }
}

impl ReceiverRunClaim {
    pub(in crate::state::receiver) const fn new(
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
    pub(in crate::state::receiver) const fn new(
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
#[derive(Clone, PartialEq, Eq)]
pub struct ReceiverClaim {
    job_id: ReceiverJobId,
    owner: String,
    expires_at_unix_ms: u64,
}

impl std::fmt::Debug for ReceiverClaim {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiverClaim(<redacted>)")
    }
}

impl ReceiverClaim {
    pub(in crate::state::receiver) fn new(
        job_id: ReceiverJobId,
        owner: String,
        expires_at_unix_ms: u64,
    ) -> Self {
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
