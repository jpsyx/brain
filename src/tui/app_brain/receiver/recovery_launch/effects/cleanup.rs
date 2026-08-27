//! Shutdown-first cleanup for one successfully spawned recovery controller.

use crate::state::ReceiverRecoveryCleanupOutcome;
use crate::tui::App;
use crate::tui::receiver::{
    DurableReceiverRun, ReceiverCleanupAuthority, SpawnedRecoveryRun, SpawnedRecoveryStage,
};

impl App {
    pub(in crate::tui::app_brain::receiver) fn continue_spawned_recovery_cleanup(
        &mut self,
        mut run: SpawnedRecoveryRun,
    ) {
        if !run.shutdown_complete {
            #[cfg(test)]
            if self
                .receiver
                .take_cleanup_failure(crate::tui::receiver::ReceiverCleanupBoundary::Shutdown)
            {
                self.defer_spawned_recovery(run);
                return;
            }
            let shutdown = match &mut run.stage {
                SpawnedRecoveryStage::CleanupDetached(controller) => {
                    controller.shutdown().map(|()| true)
                }
                SpawnedRecoveryStage::CleanupTabbed(tab_id) => self.brain.shutdown_receiver_run(
                    *tab_id,
                    run.claimed.claim.job().id(),
                    run.attribution.instance(),
                ),
                SpawnedRecoveryStage::PostSpawnOwner(_)
                | SpawnedRecoveryStage::PostAllocationOwner(_) => Ok(false),
            };
            if shutdown.is_ok_and(|stopped| stopped) {
                run.shutdown_complete = true;
            } else {
                self.defer_spawned_recovery(run);
                return;
            }
        }

        if let SpawnedRecoveryStage::CleanupTabbed(tab_id) = run.stage {
            if self
                .brain
                .remove_shutdown_receiver_run(
                    tab_id,
                    run.claimed.claim.job().id(),
                    run.attribution.instance(),
                )
                .is_none()
            {
                self.defer_spawned_recovery(run);
                return;
            }
            run.stage = SpawnedRecoveryStage::PostAllocationOwner(tab_id);
        }

        if !run.artifacts_removed {
            #[cfg(test)]
            if self
                .receiver
                .take_cleanup_failure(crate::tui::receiver::ReceiverCleanupBoundary::Artifacts)
            {
                self.defer_spawned_recovery(run);
                return;
            }
            if self
                .cleanup_receiver_instance_files_checked(run.attribution.instance())
                .is_err()
            {
                self.defer_spawned_recovery(run);
                return;
            }
            run.artifacts_removed = true;
        }

        if matches!(run.cleanup_authority, ReceiverCleanupAuthority::Unresolved) {
            let now = self.receiver_now_unix_ms();
            match self.services.establish_receiver_spawned_recovery_cleanup(
                run.claimed.claim.job().id(),
                run.claimed.claim.job().token(),
                run.claimed.claim.claim().owner(),
                &run.attribution,
                run.pid,
                now,
            ) {
                Ok(ReceiverRecoveryCleanupOutcome::Exact(effect)) => {
                    run.cleanup_authority = ReceiverCleanupAuthority::Exact(effect);
                }
                Ok(ReceiverRecoveryCleanupOutcome::Changed) | Err(_) => {
                    self.defer_spawned_recovery(run);
                    return;
                }
            }
        }
        #[cfg(test)]
        if self
            .receiver
            .take_cleanup_failure(crate::tui::receiver::ReceiverCleanupBoundary::Acknowledgement)
        {
            self.defer_spawned_recovery(run);
            return;
        }
        let ReceiverCleanupAuthority::Exact(effect) = &run.cleanup_authority else {
            unreachable!("spawned cleanup authority was established above");
        };
        let now = self.receiver_now_unix_ms();
        if !matches!(
            self.services
                .acknowledge_receiver_recovery_cleanup(effect, now),
            Ok(true)
        ) {
            self.defer_spawned_recovery(run);
        }
    }

    fn defer_spawned_recovery(&mut self, mut run: SpawnedRecoveryRun) {
        run.defer_once = true;
        self.receiver
            .store_durable_run(DurableReceiverRun::RecoverySpawned(run));
    }
}
