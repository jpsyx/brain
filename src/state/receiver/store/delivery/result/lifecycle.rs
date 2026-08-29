use crate::state::{
    Db, ReceiverDeliveryAmbiguity, ReceiverDeliveryDecision, ReceiverDeliveryErrorCategory,
};

pub(in crate::state::receiver::store::delivery) struct DeliveryLifecycle {
    delivery_phase: crate::logging::ReceiverDeliveryPhase,
    reason: crate::logging::ReceiverLifecycleReason,
    terminal_phase: Option<crate::logging::ReceiverLifecyclePhase>,
}

impl DeliveryLifecycle {
    pub(super) const fn new(
        delivery_state: &str,
        job_state: &str,
        reason: crate::logging::ReceiverLifecycleReason,
    ) -> Self {
        Self {
            delivery_phase: match delivery_state.as_bytes() {
                b"acknowledged" => crate::logging::ReceiverDeliveryPhase::Acknowledged,
                b"retrying" => crate::logging::ReceiverDeliveryPhase::Retrying,
                b"ambiguous" => crate::logging::ReceiverDeliveryPhase::Ambiguous,
                _ => crate::logging::ReceiverDeliveryPhase::Failed,
            },
            reason,
            terminal_phase: match job_state.as_bytes() {
                b"done" => Some(crate::logging::ReceiverLifecyclePhase::Done),
                b"failed" => Some(crate::logging::ReceiverLifecyclePhase::Failed),
                _ => None,
            },
        }
    }

    pub(in crate::state::receiver::store::delivery) fn log(self, db: &Db) {
        crate::logging::log_receiver_lifecycle(
            crate::logging::ReceiverLifecycleEvent::delivery_result(
                self.delivery_phase,
                self.reason,
            ),
        );
        if let Some(phase) = self.terminal_phase {
            db.log_receiver_summary(|summary| {
                crate::logging::ReceiverLifecycleEvent::terminal(
                    phase,
                    summary.agent_queue_depth(),
                    self.reason,
                )
            });
        }
    }
}

pub(super) const fn reason(
    decision: &ReceiverDeliveryDecision,
) -> crate::logging::ReceiverLifecycleReason {
    match decision {
        ReceiverDeliveryDecision::Acknowledged(_) => {
            crate::logging::ReceiverLifecycleReason::ProviderAcknowledged
        }
        ReceiverDeliveryDecision::RetryAt { error_category, .. }
        | ReceiverDeliveryDecision::TerminalFailure(error_category) => {
            error_reason(*error_category)
        }
        ReceiverDeliveryDecision::TerminalAmbiguous(reason) => ambiguity_reason(*reason),
    }
}

const fn error_reason(
    category: ReceiverDeliveryErrorCategory,
) -> crate::logging::ReceiverLifecycleReason {
    match category {
        ReceiverDeliveryErrorCategory::Authorization => {
            crate::logging::ReceiverLifecycleReason::Authorization
        }
        ReceiverDeliveryErrorCategory::Credentials => {
            crate::logging::ReceiverLifecycleReason::Credentials
        }
        ReceiverDeliveryErrorCategory::InvalidRequest => {
            crate::logging::ReceiverLifecycleReason::InvalidRequest
        }
        ReceiverDeliveryErrorCategory::ProviderRejected => {
            crate::logging::ReceiverLifecycleReason::ProviderRejected
        }
        ReceiverDeliveryErrorCategory::TransportUnavailable => {
            crate::logging::ReceiverLifecycleReason::TransportUnavailable
        }
        ReceiverDeliveryErrorCategory::RetryExhausted => {
            crate::logging::ReceiverLifecycleReason::RetryExhausted
        }
        ReceiverDeliveryErrorCategory::IdempotencyWindowExpired => {
            crate::logging::ReceiverLifecycleReason::IdempotencyWindowExpired
        }
    }
}

const fn ambiguity_reason(
    reason: ReceiverDeliveryAmbiguity,
) -> crate::logging::ReceiverLifecycleReason {
    match reason {
        ReceiverDeliveryAmbiguity::ProviderAcceptanceUnknown => {
            crate::logging::ReceiverLifecycleReason::ProviderAcceptanceUnknown
        }
        ReceiverDeliveryAmbiguity::ProviderAcknowledgementMalformed => {
            crate::logging::ReceiverLifecycleReason::ProviderAcknowledgementMalformed
        }
        ReceiverDeliveryAmbiguity::ResultCommitUnknown => {
            crate::logging::ReceiverLifecycleReason::ResultCommitUnknown
        }
        ReceiverDeliveryAmbiguity::IdempotencyWindowExpired => {
            crate::logging::ReceiverLifecycleReason::IdempotencyWindowExpired
        }
    }
}
