//! Durable reconciliation effects executed against exact local receiver state.

use crate::state::{ReceiverReconciliationAction, ReceiverReconciliationEffect};
use crate::tui::App;
use crate::tui::receiver::{
    ActiveReceiverRun, ClaimedReceiverRun, CleanupPendingReceiverRun, DurableReceiverRun,
    ReceiverCleanupAuthority,
};

impl App {
    pub(super) fn reconcile_receiver_job(&mut self) {
        let now = self.receiver_now_unix_ms();
        match self.services.reconcile_next_receiver_job(now) {
            Ok(Some(effect)) => self.execute_receiver_reconciliation_effect(&effect),
            Ok(None) => {}
            Err(_) => crate::logging::log("receiver reconciliation failed boundary=durable-store"),
        }
    }

    fn execute_receiver_reconciliation_effect(&mut self, effect: &ReceiverReconciliationEffect) {
        let run = self.receiver.take_durable_run();
        match run {
            DurableReceiverRun::Active(active) if self.effect_matches_active(effect, &active) => {
                self.continue_receiver_cleanup(CleanupPendingReceiverRun {
                    active,
                    effect: effect.clone(),
                    shutdown_complete: false,
                    artifacts_removed: false,
                    defer_once: false,
                });
            }
            DurableReceiverRun::CleanupPending(pending) if pending.effect == *effect => {
                self.continue_receiver_cleanup(pending);
            }
            DurableReceiverRun::Claimed(claimed) if effect_matches_claimed(effect, &claimed) => {
                self.services
                    .cancel_receiver_attachment_stage(claimed.claim.job().id());
                if self
                    .cleanup_receiver_instance_files_checked(claimed.identity.instance())
                    .is_err()
                {
                    self.receiver
                        .store_durable_run(DurableReceiverRun::Claimed(claimed));
                }
            }
            DurableReceiverRun::RecoveryClaimed(claimed)
                if effect_matches_claimed(effect, &claimed) =>
            {
                self.services
                    .cancel_receiver_attachment_stage(claimed.claim.job().id());
                if self
                    .cleanup_receiver_instance_files_checked(claimed.identity.instance())
                    .is_err()
                {
                    self.receiver
                        .store_durable_run(DurableReceiverRun::RecoveryClaimed(claimed));
                }
            }
            DurableReceiverRun::RecoveryPreSpawnCleanup(mut cleanup)
                if effect_matches_pre_spawn(effect, &cleanup) =>
            {
                let first_effect = matches!(
                    cleanup.cleanup_authority,
                    ReceiverCleanupAuthority::Unresolved
                );
                cleanup.cleanup_authority = ReceiverCleanupAuthority::Exact(effect.clone());
                if first_effect {
                    cleanup.defer_once = true;
                }
                self.receiver
                    .store_durable_run(DurableReceiverRun::RecoveryPreSpawnCleanup(cleanup));
            }
            DurableReceiverRun::RecoveryPreSpawnCleanup(cleanup) => self
                .receiver
                .store_durable_run(DurableReceiverRun::RecoveryPreSpawnCleanup(cleanup)),
            DurableReceiverRun::RecoverySpawned(mut spawned)
                if effect_matches_spawned(effect, &spawned) =>
            {
                let first_effect = matches!(
                    spawned.cleanup_authority,
                    ReceiverCleanupAuthority::Unresolved
                );
                spawned.cleanup_authority = ReceiverCleanupAuthority::Exact(effect.clone());
                if first_effect {
                    spawned.defer_once = true;
                }
                self.receiver
                    .store_durable_run(DurableReceiverRun::RecoverySpawned(spawned));
            }
            other => {
                self.cleanup_reconciled_absent(effect);
                self.receiver.store_durable_run(other);
            }
        }
    }

    fn cleanup_reconciled_absent(&self, effect: &ReceiverReconciliationEffect) {
        if effect.action() == ReceiverReconciliationAction::RequeuePreAcceptance {
            return;
        }
        match self.services.receiver_cleanup_registration_is_stale(effect) {
            Ok(true) => {}
            Ok(false) => return,
            Err(_) => {
                crate::logging::log(format!(
                    "receiver recovery cleanup incomplete job={} boundary=stale-proof reason=store-error",
                    effect.job_id()
                ));
                return;
            }
        }
        let Some(instance) = effect.cleanup_instance() else {
            return;
        };
        if self
            .cleanup_receiver_instance_files_checked(instance)
            .is_err()
        {
            crate::logging::log(format!(
                "receiver recovery cleanup incomplete job={} boundary=artifacts reason=filesystem-error",
                effect.job_id()
            ));
            return;
        }
        let now = self.receiver_now_unix_ms();
        match self
            .services
            .acknowledge_receiver_recovery_cleanup(effect, now)
        {
            Ok(true) => {}
            Ok(false) => crate::logging::log(format!(
                "receiver recovery cleanup incomplete job={} boundary=acknowledgement reason=stale-proof-changed",
                effect.job_id()
            )),
            Err(_) => crate::logging::log(format!(
                "receiver recovery cleanup incomplete job={} boundary=acknowledgement reason=store-error",
                effect.job_id()
            )),
        }
    }

    fn effect_matches_active(
        &self,
        effect: &ReceiverReconciliationEffect,
        active: &ActiveReceiverRun,
    ) -> bool {
        if active.claim.job().id() != effect.job_id()
            || active.claim.job().token() != effect.token()
            || effect.cleanup_instance() != Some(active.attribution.instance())
        {
            return false;
        }
        let Some(expected_session) = effect.cleanup_session_id() else {
            return false;
        };
        active.attribution.registered_session().as_str() == expected_session
            || self
                .services
                .locked_session_for_instance(
                    active.attribution.instance(),
                    active.attribution.scope(),
                )
                .as_deref()
                == Some(expected_session)
    }

    pub(super) fn continue_receiver_cleanup(&mut self, mut pending: CleanupPendingReceiverRun) {
        if !pending.shutdown_complete {
            #[cfg(test)]
            if self
                .receiver
                .take_cleanup_failure(crate::tui::receiver::ReceiverCleanupBoundary::Shutdown)
            {
                self.defer_receiver_cleanup(pending);
                return;
            }
            match self.brain.shutdown_receiver_run(
                pending.active.tab_id,
                pending.effect.job_id(),
                pending.active.attribution.instance(),
            ) {
                Ok(true) => pending.shutdown_complete = true,
                Ok(false) => {
                    self.defer_receiver_cleanup(pending);
                    return;
                }
                Err(_) => {
                    crate::logging::log(format!(
                        "receiver recovery cleanup incomplete job={} boundary=shutdown reason=controller-error",
                        pending.effect.job_id()
                    ));
                    self.defer_receiver_cleanup(pending);
                    return;
                }
            }
        }
        if !pending.artifacts_removed {
            #[cfg(test)]
            if self
                .receiver
                .take_cleanup_failure(crate::tui::receiver::ReceiverCleanupBoundary::Artifacts)
            {
                self.defer_receiver_cleanup(pending);
                return;
            }
            if self
                .cleanup_receiver_instance_files_checked(pending.active.attribution.instance())
                .is_err()
            {
                crate::logging::log(format!(
                    "receiver recovery cleanup incomplete job={} boundary=artifacts reason=filesystem-error",
                    pending.effect.job_id()
                ));
                self.defer_receiver_cleanup(pending);
                return;
            }
            pending.artifacts_removed = true;
        }
        if pending.effect.action() != ReceiverReconciliationAction::RequeuePreAcceptance {
            #[cfg(test)]
            if self.receiver.take_cleanup_failure(
                crate::tui::receiver::ReceiverCleanupBoundary::Acknowledgement,
            ) {
                self.defer_receiver_cleanup(pending);
                return;
            }
            let now = self.receiver_now_unix_ms();
            match self
                .services
                .acknowledge_receiver_recovery_cleanup(&pending.effect, now)
            {
                Ok(true) => {}
                Ok(false) => {
                    self.defer_receiver_cleanup(pending);
                    return;
                }
                Err(_) => {
                    crate::logging::log(format!(
                        "receiver recovery cleanup incomplete job={} boundary=acknowledgement reason=store-error",
                        pending.effect.job_id()
                    ));
                    self.defer_receiver_cleanup(pending);
                    return;
                }
            }
        }
        if self
            .brain
            .remove_shutdown_receiver_run(
                pending.active.tab_id,
                pending.effect.job_id(),
                pending.active.attribution.instance(),
            )
            .is_none()
        {
            self.defer_receiver_cleanup(pending);
        }
    }

    fn defer_receiver_cleanup(&mut self, mut pending: CleanupPendingReceiverRun) {
        pending.defer_once = true;
        self.receiver
            .store_durable_run(DurableReceiverRun::CleanupPending(pending));
    }
}

fn effect_matches_pre_spawn(
    effect: &ReceiverReconciliationEffect,
    cleanup: &crate::tui::receiver::PreSpawnRecoveryCleanup,
) -> bool {
    cleanup.claimed.claim.job().id() == effect.job_id()
        && cleanup.claimed.claim.job().token() == effect.token()
        && cleanup.attribution.as_ref().is_some_and(|attribution| {
            effect.cleanup_instance() == Some(attribution.instance())
                && effect.cleanup_session_id() == Some(attribution.registered_session().as_str())
        })
}

fn effect_matches_claimed(
    effect: &ReceiverReconciliationEffect,
    claimed: &ClaimedReceiverRun,
) -> bool {
    claimed.claim.job().id() == effect.job_id()
        && claimed.claim.job().token() == effect.token()
        && effect.cleanup_instance() == Some(claimed.identity.instance())
        && effect.cleanup_session_id().is_none()
}

fn effect_matches_spawned(
    effect: &ReceiverReconciliationEffect,
    spawned: &crate::tui::receiver::SpawnedRecoveryRun,
) -> bool {
    spawned.claimed.claim.job().id() == effect.job_id()
        && spawned.claimed.claim.job().token() == effect.token()
        && effect.cleanup_instance() == Some(spawned.attribution.instance())
        && effect.cleanup_session_id() == Some(spawned.attribution.registered_session().as_str())
}
