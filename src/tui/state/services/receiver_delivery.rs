use super::AppServices;

use crate::server::delivery::{
    ReceiverDeliveryExecution, ReceiverDeliveryExecutionPoll, ReceiverProviderProcessFailure,
    classify_provider_process_failure,
};
use crate::state::{ReceiverDeliveryApplyOutcome, ReceiverDeliveryClaim};
use crate::workspace::CommandContext;

pub(super) struct UnavailableReceiverDeliveryExecution;

impl ReceiverDeliveryExecution for UnavailableReceiverDeliveryExecution {
    fn reserve(
        &mut self,
        _command: CommandContext,
        claim: ReceiverDeliveryClaim,
    ) -> Result<Box<dyn crate::server::delivery::ReceiverDeliveryStart>, Box<ReceiverDeliveryClaim>>
    {
        Err(Box::new(claim))
    }

    fn poll(&self) -> ReceiverDeliveryExecutionPoll {
        ReceiverDeliveryExecutionPoll::Disconnected
    }

    fn cancel(&mut self) {}
}

impl AppServices {
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
        match self.db.reconcile_expired_receiver_deliveries(now_unix_ms) {
            Ok(_) => {
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
                return;
            }
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
            Ok(ReceiverDeliveryApplyOutcome::Applied | ReceiverDeliveryApplyOutcome::Stale) => {}
            Err(error) => {
                crate::logging::log(format!("receiver delivery result commit failed: {error:#}"));
            }
        }
        if self.receiver_delivery_active.as_ref() == Some(&claim) {
            self.receiver_delivery_active = None;
        }
    }
}
