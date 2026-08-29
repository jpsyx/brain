use crate::state::{
    Db, ReceiverDeliveryAmbiguity, ReceiverDeliveryDecision, ReceiverDeliveryErrorCategory,
};

pub(in crate::state::receiver::store::delivery) struct DeliveryLifecycle {
    delivery_phase: crate::logging::ReceiverDeliveryPhase,
    reason: crate::logging::ReceiverLifecycleReason,
    terminal_phase: Option<crate::logging::ReceiverLifecyclePhase>,
}

impl DeliveryLifecycle {
    pub(in crate::state::receiver::store::delivery) fn new(
        delivery_state: &str,
        job_state: &str,
        reason: crate::logging::ReceiverLifecycleReason,
    ) -> anyhow::Result<Self> {
        let delivery_phase = match delivery_state {
            "acknowledged" => crate::logging::ReceiverDeliveryPhase::Acknowledged,
            "retrying" => crate::logging::ReceiverDeliveryPhase::Retrying,
            "ambiguous" => crate::logging::ReceiverDeliveryPhase::Ambiguous,
            "failed" => crate::logging::ReceiverDeliveryPhase::Failed,
            _ => anyhow::bail!("unknown receiver delivery lifecycle state"),
        };
        let terminal_phase = match job_state {
            "answer-ready" => Some(crate::logging::ReceiverLifecyclePhase::AnswerReady),
            "done" => Some(crate::logging::ReceiverLifecyclePhase::Done),
            "failed" => Some(crate::logging::ReceiverLifecyclePhase::Failed),
            "retrying" => None,
            _ => anyhow::bail!("unknown receiver job lifecycle state"),
        };
        Ok(Self {
            delivery_phase,
            reason,
            terminal_phase,
        })
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
                    summary.map(crate::state::ReceiverWorkSummary::agent_queue_depth),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_durable_states_are_rejected_instead_of_fabricating_lifecycle_facts() {
        let reason = crate::logging::ReceiverLifecycleReason::InvalidRequest;

        assert!(
            DeliveryLifecycle::new("unknown-delivery", "failed", reason).is_err(),
            "unknown delivery state was accepted"
        );
        assert!(
            DeliveryLifecycle::new("failed", "unknown-job", reason).is_err(),
            "unknown job state was accepted"
        );
    }
}
