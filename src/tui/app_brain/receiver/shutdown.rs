//! Orderly cleanup for receiver work before generic controller shutdown.

use crate::state::ReceiverJobState;
use crate::state::ReceiverLaunchFailure;
use crate::tui::App;
use crate::tui::receiver::{
    ActiveReceiverRun, ClaimedReceiverRun, CleanupPendingReceiverRun, DurableReceiverRun,
    ReceiverCleanupAuthority, SpawnedRecoveryRun, SpawnedRecoveryStage,
};

use super::diagnostic::receiver_observation_diagnostic;

impl App {
    pub(crate) fn shutdown_receiver_runtime(&mut self) {
        let answer_cleanup_attempts = self.receiver.answer_controller_cleanup_count();
        for _ in 0..answer_cleanup_attempts {
            self.continue_oldest_receiver_answer_controller_cleanup();
        }
        match self.receiver.take_durable_run() {
            DurableReceiverRun::Idle => self.services.shutdown_receiver_attachments(),
            DurableReceiverRun::Claimed(claimed) => self.shutdown_claimed_receiver_run(&claimed),
            DurableReceiverRun::RecoveryClaimed(claimed) => {
                self.services
                    .cancel_receiver_attachment_stage(claimed.claim.job().id());
                self.services.shutdown_receiver_attachments();
            }
            DurableReceiverRun::RecoveryPreSpawnCleanup(mut cleanup) => {
                self.services.shutdown_receiver_attachments();
                if !cleanup.shutdown_complete {
                    let _ = cleanup.controller.shutdown();
                }
                if cleanup.shutdown_complete
                    && matches!(
                        cleanup.cleanup_authority,
                        ReceiverCleanupAuthority::Unresolved
                    )
                    && let Some(attribution) = cleanup.attribution.as_ref()
                {
                    let _ = self.services.release_receiver_session(attribution);
                }
                crate::logging::log(
                    "receiver shutdown preserved pre-spawn recovery cleanup authority",
                );
            }
            DurableReceiverRun::RecoverySpawned(spawned) => {
                self.shutdown_spawned_recovery_run(spawned);
            }
            DurableReceiverRun::Active(active) => self.shutdown_active_receiver_run(active),
            DurableReceiverRun::AnswerCleanupPending(active) => {
                self.shutdown_answer_cleanup_pending_receiver_run(&active);
            }
            DurableReceiverRun::CleanupPending(pending) => {
                self.shutdown_cleanup_pending_receiver_run(&pending);
            }
        }
    }

    fn shutdown_spawned_recovery_run(&mut self, mut spawned: SpawnedRecoveryRun) {
        self.services.shutdown_receiver_attachments();
        let shutdown = if spawned.shutdown_complete {
            Ok(true)
        } else {
            match &mut spawned.stage {
                SpawnedRecoveryStage::PostSpawnOwner(controller)
                | SpawnedRecoveryStage::CleanupDetached(controller) => {
                    controller.shutdown().map(|()| true)
                }
                SpawnedRecoveryStage::PostAllocationOwner(tab_id)
                | SpawnedRecoveryStage::CleanupTabbed(tab_id) => self.brain.shutdown_receiver_run(
                    *tab_id,
                    spawned.claimed.claim.job().id(),
                    spawned.attribution.instance(),
                ),
            }
        };
        if shutdown != Ok(true) {
            crate::logging::log(format!(
                "receiver shutdown preserved spawned recovery capability for restart cleanup pid={}",
                spawned.pid
            ));
            return;
        }
        if let SpawnedRecoveryStage::PostAllocationOwner(tab_id)
        | SpawnedRecoveryStage::CleanupTabbed(tab_id) = spawned.stage
        {
            let _ = self.brain.remove_shutdown_receiver_run(
                tab_id,
                spawned.claimed.claim.job().id(),
                spawned.attribution.instance(),
            );
        }
        self.cleanup_receiver_instance_files(spawned.attribution.instance());
        if matches!(
            spawned.cleanup_authority,
            ReceiverCleanupAuthority::Unresolved
        ) {
            let now = self.receiver_now_unix_ms();
            let _ = self.services.establish_receiver_spawned_recovery_cleanup(
                spawned.claimed.claim.job().id(),
                spawned.claimed.claim.job().token(),
                spawned.claimed.claim.claim().owner(),
                &spawned.attribution,
                spawned.pid,
                now,
            );
        }
        crate::logging::log("receiver shutdown preserved spawned recovery durable evidence");
    }

    fn shutdown_claimed_receiver_run(&mut self, claimed: &ClaimedReceiverRun) {
        self.services
            .cancel_receiver_attachment_stage(claimed.claim.job().id());
        self.services.shutdown_receiver_attachments();
        match self.retry_receiver_owner_now(&claimed.claim, ReceiverLaunchFailure::Planning) {
            Ok(Some(_)) => {}
            Ok(None) => crate::logging::log(
                "receiver planning shutdown occurred after durable ownership changed",
            ),
            Err(error) => crate::logging::log(format!(
                "receiver planning shutdown retry failed: {error:#}"
            )),
        }
    }

    fn shutdown_active_receiver_run(&mut self, active: ActiveReceiverRun) {
        let ActiveReceiverRun {
            claim,
            attribution,
            tab_id,
            _attachments: attachments,
        } = active;
        self.services.shutdown_receiver_attachments();
        let removed = self.brain.remove_receiver_run(tab_id);
        if removed.as_ref().is_some_and(|removed| {
            removed.job_id != claim.job().id() || removed.instance != attribution.instance()
        }) {
            let prior = self
                .services
                .receiver_observation_cursor(claim.job().id())
                .ok()
                .flatten()
                .map_or(ReceiverJobState::Launched, |(state, _)| state);
            crate::logging::log(receiver_observation_diagnostic(
                claim.job().id(),
                attribution.instance(),
                attribution.scope().agent_kind(),
                prior,
                None,
                "tab-shutdown-identity-mismatch",
            ));
        }
        self.cleanup_receiver_instance_files(attribution.instance());
        drop(attachments);
        crate::logging::log("receiver shutdown preserved launched durable evidence");
    }

    fn shutdown_answer_cleanup_pending_receiver_run(&mut self, active: &ActiveReceiverRun) {
        self.services.shutdown_receiver_attachments();
        let shutdown = self.brain.shutdown_receiver_run(
            active.tab_id,
            active.claim.job().id(),
            active.attribution.instance(),
        );
        if shutdown != Ok(true) {
            crate::logging::log(
                "receiver shutdown preserved answer cleanup controller and durable authority",
            );
            return;
        }
        let controller_pid = i32::try_from(std::process::id()).unwrap_or(0);
        let acknowledged = self
            .services
            .acknowledge_receiver_answer_controller_shutdown(
                active.claim.job().id(),
                active.claim.job().token(),
                active.attribution.instance(),
                controller_pid,
                self.receiver_now_unix_ms(),
            )
            .unwrap_or(false);
        if !acknowledged {
            crate::logging::log(
                "receiver shutdown preserved confirmed answer cleanup controller handoff",
            );
            return;
        }
        let job_id = active.claim.job().id();
        if self
            .brain
            .remove_shutdown_receiver_run(active.tab_id, job_id, active.attribution.instance())
            .is_none()
        {
            crate::logging::log(
                "receiver shutdown could not remove the confirmed answer cleanup controller",
            );
            return;
        }
        #[cfg(test)]
        self.receiver.record_answer_cleanup_event(
            crate::tui::receiver::ReceiverAnswerCleanupEvent::ControllerShutdown,
        );
        self.continue_receiver_answer_cleanup_for(job_id);
    }

    fn shutdown_cleanup_pending_receiver_run(&mut self, pending: &CleanupPendingReceiverRun) {
        self.services.shutdown_receiver_attachments();
        if !pending.shutdown_complete {
            let _ = self.brain.shutdown_receiver_run(
                pending.active.tab_id,
                pending.active.claim.job().id(),
                pending.active.attribution.instance(),
            );
        }
        let _ = self.brain.remove_shutdown_receiver_run(
            pending.active.tab_id,
            pending.active.claim.job().id(),
            pending.active.attribution.instance(),
        );
        if !pending.artifacts_removed {
            self.cleanup_receiver_instance_files(pending.active.attribution.instance());
        }
        crate::logging::log("receiver shutdown preserved cleanup-fenced durable evidence");
    }
}
