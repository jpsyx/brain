use super::ReceiverDeliveryCounts;

/// Finite durable agent-work phase safe for status and diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverWorkPhase {
    Queued,
    Claimed,
    Launching,
    Launched,
    Accepted,
    Processing,
    Retrying,
}

impl ReceiverWorkPhase {
    pub(in crate::state::receiver) fn parse(value: &str) -> Option<Self> {
        match value {
            "queued" => Some(Self::Queued),
            "claimed" => Some(Self::Claimed),
            "launching" => Some(Self::Launching),
            "launched" => Some(Self::Launched),
            "accepted" => Some(Self::Accepted),
            "processing" => Some(Self::Processing),
            "retrying" => Some(Self::Retrying),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Claimed => "claimed",
            Self::Launching => "launching",
            Self::Launched => "launched",
            Self::Accepted => "accepted",
            Self::Processing => "processing",
            Self::Retrying => "retrying",
        }
    }
}

/// One content-free snapshot of durable receiver work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReceiverWorkSummary {
    agent_queue_depth: usize,
    oldest_active_phase: Option<ReceiverWorkPhase>,
    recovery_attempt: Option<u32>,
    recovery_limit: u32,
    cleanup_gated_responses: usize,
    delivery_counts: ReceiverDeliveryCounts,
}

impl ReceiverWorkSummary {
    #[must_use]
    pub(in crate::state::receiver) const fn new(
        agent_queue_depth: usize,
        oldest_active_phase: Option<ReceiverWorkPhase>,
        recovery_attempt: Option<u32>,
        recovery_limit: u32,
        cleanup_gated_responses: usize,
        delivery_counts: ReceiverDeliveryCounts,
    ) -> Self {
        Self {
            agent_queue_depth,
            oldest_active_phase,
            recovery_attempt,
            recovery_limit,
            cleanup_gated_responses,
            delivery_counts,
        }
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) const fn new_for_test(
        agent_queue_depth: usize,
        oldest_active_phase: Option<ReceiverWorkPhase>,
        recovery_attempt: Option<u32>,
        recovery_limit: u32,
        cleanup_gated_responses: usize,
        delivery_counts: ReceiverDeliveryCounts,
    ) -> Self {
        Self::new(
            agent_queue_depth,
            oldest_active_phase,
            recovery_attempt,
            recovery_limit,
            cleanup_gated_responses,
            delivery_counts,
        )
    }

    #[must_use]
    pub const fn agent_queue_depth(self) -> usize {
        self.agent_queue_depth
    }

    #[must_use]
    pub const fn oldest_active_phase(self) -> Option<ReceiverWorkPhase> {
        self.oldest_active_phase
    }

    #[must_use]
    pub const fn recovery_attempt(self) -> Option<u32> {
        self.recovery_attempt
    }

    #[must_use]
    pub const fn recovery_limit(self) -> u32 {
        self.recovery_limit
    }

    #[must_use]
    pub const fn cleanup_gated_responses(self) -> usize {
        self.cleanup_gated_responses
    }

    #[must_use]
    pub const fn delivery_counts(self) -> ReceiverDeliveryCounts {
        self.delivery_counts
    }
}
