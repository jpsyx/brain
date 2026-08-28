use super::AppServices;

use crate::server::delivery::{
    ReceiverDeliveryExecution, ReceiverDeliveryExecutionPoll, ReceiverProviderProcessFailure,
    classify_provider_process_failure,
};
use crate::state::{ReceiverDeliveryApplyOutcome, ReceiverDeliveryClaim};
use crate::workspace::CommandContext;

#[derive(Default)]
pub(super) struct UnavailableReceiverDeliveryExecution {
    completed: std::sync::Arc<std::sync::Mutex<Option<ReceiverDeliveryClaim>>>,
}

struct UnavailableReceiverDeliveryStart {
    completed: std::sync::Arc<std::sync::Mutex<Option<ReceiverDeliveryClaim>>>,
    claim: ReceiverDeliveryClaim,
}

impl crate::server::delivery::ReceiverDeliveryStart for UnavailableReceiverDeliveryStart {
    fn start(self: Box<Self>) -> anyhow::Result<()> {
        let mut completed = self
            .completed
            .lock()
            .map_err(|_| anyhow::anyhow!("unavailable delivery result lock was poisoned"))?;
        *completed = Some(self.claim);
        drop(completed);
        Ok(())
    }
}

impl ReceiverDeliveryExecution for UnavailableReceiverDeliveryExecution {
    fn reserve(
        &mut self,
        _command: CommandContext,
        claim: ReceiverDeliveryClaim,
    ) -> Result<Box<dyn crate::server::delivery::ReceiverDeliveryStart>, Box<ReceiverDeliveryClaim>>
    {
        Ok(Box::new(UnavailableReceiverDeliveryStart {
            completed: self.completed.clone(),
            claim,
        }))
    }

    fn poll(&self) -> ReceiverDeliveryExecutionPoll {
        let Ok(mut completed) = self.completed.lock() else {
            return ReceiverDeliveryExecutionPoll::Disconnected;
        };
        let Some(claim) = completed.take() else {
            return ReceiverDeliveryExecutionPoll::Pending;
        };
        ReceiverDeliveryExecutionPoll::Ready {
            claim: Box::new(claim),
            result: crate::state::ReceiverProviderResultClass::DefinitelyNotAccepted(
                crate::state::ReceiverDeliveryErrorCategory::TransportUnavailable,
            ),
        }
    }

    fn cancel(&mut self) {}
}

impl AppServices {
    fn log_receiver_delivery_state(&self, stage: &'static str, changed: usize) {
        match self.db.receiver_delivery_counts() {
            Ok(counts) => {
                crate::logging::log(receiver_delivery_state_diagnostic(stage, changed, counts));
            }
            Err(_) => crate::logging::log(format!(
                "receiver delivery stage={stage} boundary=status-counts category=unavailable"
            )),
        }
    }

    pub(crate) fn cancel_receiver_delivery(&mut self) {
        self.receiver_delivery_execution.cancel();
    }

    pub(crate) fn tick_receiver_delivery(
        &mut self,
        command: &CommandContext,
        owner: &str,
        now_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) {
        self.apply_receiver_delivery_execution_result(now_unix_ms);
        if !self.reconcile_receiver_delivery_state(now_unix_ms) {
            return;
        }
        if self.receiver_delivery_active.is_some() {
            return;
        }
        let claim =
            match self
                .db
                .claim_next_receiver_delivery(owner, now_unix_ms, expires_at_unix_ms)
            {
                Ok(Some(claim)) => claim,
                Ok(None) => return,
                Err(error) => {
                    crate::logging::log(format!("receiver delivery claim failed: {error:#}"));
                    return;
                }
            };
        self.log_receiver_delivery_state("claim", 1);
        let start = match self
            .receiver_delivery_execution
            .reserve(command.clone(), claim.clone())
        {
            Ok(start) => start,
            Err(claim) => {
                if let Err(error) = self
                    .db
                    .release_receiver_delivery_before_io(&claim, now_unix_ms)
                {
                    crate::logging::log(format!(
                        "receiver delivery reservation release failed: {error:#}"
                    ));
                }
                return;
            }
        };
        match self
            .db
            .mark_receiver_delivery_io_started(&claim, now_unix_ms)
        {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                crate::logging::log(format!(
                    "receiver delivery IO-start commit failed: {error:#}"
                ));
                return;
            }
        }
        if let Err(error) = start.start() {
            crate::logging::log(format!(
                "receiver delivery worker publication failed: {error:#}"
            ));
            match self
                .db
                .release_receiver_delivery_after_failed_publication(&claim, now_unix_ms)
            {
                Ok(true) => {}
                Ok(false) => crate::logging::log(
                    "receiver delivery publication release lost exact authority",
                ),
                Err(release_error) => crate::logging::log(format!(
                    "receiver delivery publication release failed: {release_error:#}"
                )),
            }
            return;
        }
        self.receiver_delivery_active = Some(claim);
    }

    pub(crate) fn reconcile_receiver_delivery_state(&mut self, now_unix_ms: u64) -> bool {
        match self.db.reconcile_expired_receiver_deliveries(now_unix_ms) {
            Ok(changed) => {
                if changed > 0 {
                    self.log_receiver_delivery_state("reconciliation", changed);
                }
                if self
                    .receiver_delivery_active
                    .as_ref()
                    .is_some_and(|claim| claim.expires_at_unix_ms() <= now_unix_ms)
                {
                    self.receiver_delivery_active = None;
                }
            }
            Err(error) => {
                crate::logging::log(format!(
                    "receiver delivery reconciliation failed: {error:#}"
                ));
                return false;
            }
        }
        true
    }

    fn apply_receiver_delivery_execution_result(&mut self, now_unix_ms: u64) {
        let (claim, result) = match self.receiver_delivery_execution.poll() {
            ReceiverDeliveryExecutionPoll::Pending => return,
            ReceiverDeliveryExecutionPoll::Ready { claim, result } => (*claim, result),
            ReceiverDeliveryExecutionPoll::Disconnected => {
                let Some(claim) = self.receiver_delivery_active.take() else {
                    return;
                };
                (
                    claim,
                    classify_provider_process_failure(
                        ReceiverProviderProcessFailure::LostResultChannel,
                    ),
                )
            }
        };
        match self
            .db
            .apply_receiver_delivery_result(&claim, now_unix_ms, result)
        {
            Ok(ReceiverDeliveryApplyOutcome::Applied) => {
                self.log_receiver_delivery_state("result", 1);
            }
            Ok(ReceiverDeliveryApplyOutcome::Stale) => {}
            Err(error) => {
                crate::logging::log(format!("receiver delivery result commit failed: {error:#}"));
            }
        }
        if self.receiver_delivery_active.as_ref() == Some(&claim) {
            self.receiver_delivery_active = None;
        }
    }
}

fn receiver_delivery_state_diagnostic(
    stage: &'static str,
    changed: usize,
    counts: crate::state::ReceiverDeliveryCounts,
) -> String {
    format!(
        "receiver delivery stage={stage} changed={changed} phases=answer-ready:{},delivering:{},retrying:{},ambiguous:{},failed:{},done:{} reasons=retry-exhausted:{},permanent-rejection:{},ambiguous-acknowledgement:{},idempotency-window-expired:{},no-safe-fallback:{}",
        counts.answer_ready(),
        counts.delivering(),
        counts.retrying(),
        counts.ambiguous(),
        counts.failed(),
        counts.done(),
        counts.retry_exhausted(),
        counts.permanent_rejection(),
        counts.ambiguous_acknowledgement(),
        counts.idempotency_window_expired(),
        counts.no_safe_fallback(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delivery_tick_diagnostic_is_stable_and_content_free() {
        let counts = crate::state::ReceiverDeliveryCounts::new(1, 2, 3, 4, 5, 6)
            .with_terminal_reasons(7, 8, 9, 10, 11);

        let diagnostic = receiver_delivery_state_diagnostic("reconciliation", 12, counts);

        assert_eq!(
            diagnostic,
            "receiver delivery stage=reconciliation changed=12 phases=answer-ready:1,delivering:2,retrying:3,ambiguous:4,failed:5,done:6 reasons=retry-exhausted:7,permanent-rejection:8,ambiguous-acknowledgement:9,idempotency-window-expired:10,no-safe-fallback:11"
        );
        for forbidden in [
            "private-sender",
            "private-recipient",
            "private-answer",
            "provider-response",
            "credential-secret",
            "mutable-instance",
        ] {
            assert!(
                !diagnostic.contains(forbidden),
                "diagnostic leaked forbidden content"
            );
        }
    }
}
