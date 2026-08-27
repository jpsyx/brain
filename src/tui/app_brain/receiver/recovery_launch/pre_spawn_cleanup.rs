//! Retryable cleanup for recovery resources created before process spawn.

use crate::agent::AgentController;
use crate::tui::receiver::{
    ClaimedReceiverRun, DurableReceiverRun, PreSpawnRecoveryCleanup, PreSpawnRecoveryOutcome,
};

pub(super) fn begin_recovery_pre_spawn_cleanup(
    receiver: &mut crate::tui::receiver::ReceiverRuntime,
    services: &crate::tui::state::AppServices,
    claimed: ClaimedReceiverRun,
    controller: AgentController,
    attribution: Option<crate::state::ReceiverSessionAttribution>,
    outcome: PreSpawnRecoveryOutcome,
) {
    continue_recovery_pre_spawn_cleanup(
        receiver,
        services,
        PreSpawnRecoveryCleanup {
            claimed,
            controller,
            attribution,
            outcome,
            shutdown_complete: false,
            defer_once: false,
        },
    );
}

pub(in crate::tui::app_brain::receiver) fn continue_recovery_pre_spawn_cleanup(
    receiver: &mut crate::tui::receiver::ReceiverRuntime,
    services: &crate::tui::state::AppServices,
    mut cleanup: PreSpawnRecoveryCleanup,
) {
    if cleanup.defer_once {
        cleanup.defer_once = false;
        receiver.store_durable_run(DurableReceiverRun::RecoveryPreSpawnCleanup(cleanup));
        return;
    }
    if !cleanup.shutdown_complete {
        #[cfg(test)]
        if receiver.take_cleanup_failure(crate::tui::receiver::ReceiverCleanupBoundary::Shutdown) {
            cleanup.defer_once = true;
            receiver.store_durable_run(DurableReceiverRun::RecoveryPreSpawnCleanup(cleanup));
            return;
        }
        if cleanup.controller.shutdown().is_err() {
            cleanup.defer_once = true;
            receiver.store_durable_run(DurableReceiverRun::RecoveryPreSpawnCleanup(cleanup));
            return;
        }
        cleanup.shutdown_complete = true;
    }
    if let Some(attribution) = cleanup.attribution.as_ref()
        && services.release_receiver_session(attribution).is_err()
    {
        cleanup.defer_once = true;
        receiver.store_durable_run(DurableReceiverRun::RecoveryPreSpawnCleanup(cleanup));
        return;
    }
    let now = u64::try_from(services.utc_now().timestamp_millis()).unwrap_or(0);
    match cleanup.outcome {
        PreSpawnRecoveryOutcome::RestoreClaim => {
            receiver.store_durable_run(DurableReceiverRun::RecoveryClaimed(cleanup.claimed));
        }
        PreSpawnRecoveryOutcome::Lost => {}
        PreSpawnRecoveryOutcome::Failure(failure) => {
            if services
                .fail_receiver_recovery_attempt(
                    cleanup.claimed.claim.job().id(),
                    cleanup.claimed.claim.claim().owner(),
                    now,
                    failure,
                )
                .is_err()
            {
                crate::logging::log("receiver recovery failed boundary=launch-failure-store");
            }
        }
        PreSpawnRecoveryOutcome::ResumeUnavailable => {
            if services
                .fail_receiver_recovery_resume(
                    cleanup.claimed.claim.job().id(),
                    cleanup.claimed.claim.claim().owner(),
                    now,
                )
                .is_err()
            {
                crate::logging::log("receiver recovery failed boundary=resume-failure-store");
            }
        }
    }
}
