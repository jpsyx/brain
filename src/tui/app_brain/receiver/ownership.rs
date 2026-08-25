//! Exact durable claim observations for receiver launch effects.

use crate::state::{ReceiverLaunchFailure, ReceiverLaunchRetryOutcome, ReceiverRunClaim};
use crate::tui::App;
use crate::tui::state::AppServices;

#[derive(Clone, Copy)]
pub(super) struct ReceiverOwnerObservation {
    observed_at_unix_ms: u64,
}

#[derive(Clone, Copy)]
pub(super) enum ReceiverOwnerBlock {
    Lost,
    Deferred,
}

impl ReceiverOwnerObservation {
    pub(super) const fn observed_at_unix_ms(self) -> u64 {
        self.observed_at_unix_ms
    }
}

impl App {
    pub(super) fn authorize_receiver_owner_now(
        &self,
        claimed: &ReceiverRunClaim,
    ) -> anyhow::Result<Option<ReceiverOwnerObservation>> {
        authorize_receiver_owner_now(&self.services, claimed)
    }

    pub(super) fn retry_receiver_owner_now(
        &self,
        claimed: &ReceiverRunClaim,
        failure: ReceiverLaunchFailure,
    ) -> anyhow::Result<Option<ReceiverLaunchRetryOutcome>> {
        retry_receiver_owner_now(&self.services, claimed, failure)
    }
}

pub(super) fn retry_receiver_owner_now(
    services: &AppServices,
    claimed: &ReceiverRunClaim,
    failure: ReceiverLaunchFailure,
) -> anyhow::Result<Option<ReceiverLaunchRetryOutcome>> {
    let now = u64::try_from(services.utc_now().timestamp_millis()).unwrap_or(0);
    services.record_receiver_launch_retry(
        claimed.job().id(),
        claimed.claim().owner(),
        now,
        now.saturating_add(super::dispatch::RETRY_DELAY_MS),
        failure,
    )
}

pub(super) fn authorize_receiver_owner_now(
    services: &AppServices,
    claimed: &ReceiverRunClaim,
) -> anyhow::Result<Option<ReceiverOwnerObservation>> {
    let now = u64::try_from(services.utc_now().timestamp_millis()).unwrap_or(0);
    services
        .renew_receiver_claim(
            claimed.job().id(),
            claimed.claim().owner(),
            now,
            now.saturating_add(super::dispatch::CLAIM_LIFETIME_MS),
        )
        .map(|owned| {
            owned.then_some(ReceiverOwnerObservation {
                observed_at_unix_ms: now,
            })
        })
}
