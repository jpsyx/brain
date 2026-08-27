//! Post-registration recovery spawn and durable launch effects.

use crate::agent::{AgentController, LaunchRequest};
use crate::state::{ReceiverRecoveryFailure, ReceiverRunClaim};
use crate::tui::receiver::{
    ClaimedReceiverRun, ReceiverCleanupAuthority, ReceiverSessionRegistration, SpawnedRecoveryRun,
    SpawnedRecoveryStage, cleanup_receiver_launch,
};

mod activation;
mod cleanup;

pub(super) fn spawn_claimed_receiver_recovery(
    services: &crate::tui::state::AppServices,
    claimed: ClaimedReceiverRun,
    registration: ReceiverSessionRegistration<'_, crate::tui::state::AppServices>,
    mut controller: AgentController,
    request: &LaunchRequest,
    pid: i32,
    after_spawn: impl FnOnce(),
) -> Option<SpawnedRecoveryRun> {
    let launch = controller.launch(request);
    after_spawn();
    if launch.is_err() {
        crate::logging::log("receiver recovery failed boundary=process-spawn");
        let failure = super::shutdown_failure_or(
            &cleanup_receiver_launch(Some(registration), &mut controller),
            ReceiverRecoveryFailure::Spawn,
        );
        fail_receiver_recovery_attempt(services, &claimed.claim, failure);
        return None;
    }

    Some(SpawnedRecoveryRun {
        claimed,
        attribution: registration.commit(),
        pid,
        stage: SpawnedRecoveryStage::PostSpawnOwner(controller),
        durable_launch_committed: false,
        cleanup_authority: ReceiverCleanupAuthority::Unresolved,
        shutdown_complete: false,
        artifacts_removed: false,
        defer_once: false,
    })
}

fn fail_receiver_recovery_attempt(
    services: &crate::tui::state::AppServices,
    claim: &ReceiverRunClaim,
    failure: ReceiverRecoveryFailure,
) {
    let now = u64::try_from(services.utc_now().timestamp_millis()).unwrap_or(0);
    if services
        .fail_receiver_recovery_attempt(claim.job().id(), claim.claim().owner(), now, failure)
        .is_err()
    {
        crate::logging::log("receiver recovery failed boundary=launch-failure-store");
    }
}
