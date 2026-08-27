//! Activation decisions for one successfully spawned recovery controller.

use crate::agent::AgentController;
use crate::state::ReceiverLaunchObservation;
use crate::tui::App;
use crate::tui::receiver::attachments::PreparedReceiverAttachments;
use crate::tui::receiver::{
    ActiveReceiverRun, ClaimedReceiverRun, DurableReceiverRun, SpawnedRecoveryRun,
    SpawnedRecoveryStage,
};

impl App {
    pub(in crate::tui::app_brain::receiver) fn continue_spawned_receiver_recovery(
        &mut self,
        mut run: SpawnedRecoveryRun,
    ) {
        if run.defer_once {
            run.defer_once = false;
            self.receiver
                .store_durable_run(DurableReceiverRun::RecoverySpawned(run));
            return;
        }
        let stage = std::mem::replace(
            &mut run.stage,
            SpawnedRecoveryStage::PostAllocationOwner(crate::tui::model::SessionTabId(0)),
        );
        match stage {
            SpawnedRecoveryStage::PostSpawnOwner(controller) => {
                self.continue_recovery_launch_commit(run, controller);
            }
            SpawnedRecoveryStage::PostAllocationOwner(tab_id) => {
                self.continue_recovery_final_owner(run, tab_id);
            }
            SpawnedRecoveryStage::CleanupDetached(controller) => {
                run.stage = SpawnedRecoveryStage::CleanupDetached(controller);
                self.continue_spawned_recovery_cleanup(run);
            }
            SpawnedRecoveryStage::CleanupTabbed(tab_id) => {
                run.stage = SpawnedRecoveryStage::CleanupTabbed(tab_id);
                self.continue_spawned_recovery_cleanup(run);
            }
        }
    }

    fn continue_recovery_launch_commit(
        &mut self,
        mut run: SpawnedRecoveryRun,
        controller: AgentController,
    ) {
        let owner = match self.recovery_owner_decision(&run.claimed) {
            super::super::RecoveryOwnerDecision::Current(owner) => owner,
            super::super::RecoveryOwnerDecision::StoreUnavailable => {
                run.stage = SpawnedRecoveryStage::PostSpawnOwner(controller);
                self.receiver
                    .store_durable_run(DurableReceiverRun::RecoverySpawned(run));
                return;
            }
            super::super::RecoveryOwnerDecision::Lost => {
                run.stage = SpawnedRecoveryStage::CleanupDetached(controller);
                self.continue_spawned_recovery_cleanup(run);
                return;
            }
        };
        let observation = ReceiverLaunchObservation {
            token: run.claimed.claim.job().token(),
            instance: run.attribution.instance().to_owned(),
            session_id: run.attribution.registered_session().as_str().to_owned(),
            observed_at_unix_ms: owner.observed_at_unix_ms(),
            authorized_at_unix_ms: owner.observed_at_unix_ms(),
        };
        #[cfg(test)]
        self.receiver.run_launch_boundary_hook(
            crate::tui::receiver::ReceiverLaunchBoundary::RecoveryLaunchCommit,
        );
        match self.services.commit_receiver_recovery_job_launch(
            run.claimed.claim.job().id(),
            run.claimed.claim.claim().owner(),
            &observation,
        ) {
            Ok(true) => run.durable_launch_committed = true,
            Ok(false) => match self.services.receiver_recovery_launch_is_exact(
                run.claimed.claim.job().id(),
                run.claimed.claim.job().token(),
                &run.attribution,
            ) {
                Ok(true) => run.durable_launch_committed = true,
                Ok(false) => {
                    run.stage = SpawnedRecoveryStage::CleanupDetached(controller);
                    self.continue_spawned_recovery_cleanup(run);
                    return;
                }
                Err(_) => {
                    run.stage = SpawnedRecoveryStage::PostSpawnOwner(controller);
                    self.receiver
                        .store_durable_run(DurableReceiverRun::RecoverySpawned(run));
                    return;
                }
            },
            Err(_) => {
                crate::logging::log("receiver recovery deferred boundary=launch-commit-store");
                run.stage = SpawnedRecoveryStage::PostSpawnOwner(controller);
                self.receiver
                    .store_durable_run(DurableReceiverRun::RecoverySpawned(run));
                return;
            }
        }

        #[cfg(test)]
        let reservation = self
            .receiver
            .take_recovery_tab_error()
            .map_or_else(|| self.brain.reserve_receiver_run(), Err);
        #[cfg(not(test))]
        let reservation = self.brain.reserve_receiver_run();
        let Ok(reservation) = reservation else {
            crate::logging::log("receiver recovery failed boundary=tab-allocation");
            run.stage = SpawnedRecoveryStage::CleanupDetached(controller);
            self.continue_spawned_recovery_cleanup(run);
            return;
        };
        let tab_id = self.brain.insert_reserved_receiver_run(
            &reservation,
            run.claimed.claim.job().id(),
            recovery_title(&run.claimed),
            run.claimed.remote.instance().to_owned(),
            controller,
        );
        #[cfg(test)]
        self.receiver
            .run_launch_boundary_hook(crate::tui::receiver::ReceiverLaunchBoundary::Allocation);
        self.continue_recovery_final_owner(run, tab_id);
    }

    fn continue_recovery_final_owner(
        &mut self,
        mut run: SpawnedRecoveryRun,
        tab_id: crate::tui::model::SessionTabId,
    ) {
        match self.recovery_owner_decision(&run.claimed) {
            super::super::RecoveryOwnerDecision::Current(_) => {
                self.receiver
                    .store_durable_run(DurableReceiverRun::Active(ActiveReceiverRun {
                        claim: run.claimed.claim,
                        attribution: run.attribution,
                        tab_id,
                        _attachments: PreparedReceiverAttachments::empty(),
                    }));
            }
            super::super::RecoveryOwnerDecision::StoreUnavailable => {
                run.stage = SpawnedRecoveryStage::PostAllocationOwner(tab_id);
                self.receiver
                    .store_durable_run(DurableReceiverRun::RecoverySpawned(run));
            }
            super::super::RecoveryOwnerDecision::Lost => {
                run.stage = SpawnedRecoveryStage::CleanupTabbed(tab_id);
                self.continue_spawned_recovery_cleanup(run);
            }
        }
    }
}

fn recovery_title(claimed: &ClaimedReceiverRun) -> String {
    format!(
        "Receiver · {}",
        match claimed.claim.job().inbound().channel {
            crate::server::receiver::Channel::Sms => "SMS",
            crate::server::receiver::Channel::Email => "Email",
        }
    )
}
