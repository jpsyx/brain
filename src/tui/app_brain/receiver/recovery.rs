//! Durable reconciliation effects executed against exact local receiver state.

use crate::state::{ReceiverReconciliationAction, ReceiverReconciliationEffect};
use crate::tui::App;
use crate::tui::receiver::{ActiveReceiverRun, ClaimedReceiverRun, DurableReceiverRun};

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
                if !self.cleanup_reconciled_active(effect, &active) {
                    self.receiver
                        .store_durable_run(DurableReceiverRun::Active(active));
                }
            }
            DurableReceiverRun::Claimed(claimed) if effect_matches_claimed(effect, &claimed) => {
                self.services
                    .cancel_receiver_attachment_stage(claimed.claim.job().id());
                if self
                    .cleanup_receiver_instance_files_checked(claimed.remote.instance())
                    .is_err()
                {
                    self.receiver
                        .store_durable_run(DurableReceiverRun::Claimed(claimed));
                }
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

    pub(super) fn cleanup_reconciled_active(
        &mut self,
        effect: &ReceiverReconciliationEffect,
        active: &ActiveReceiverRun,
    ) -> bool {
        match self.brain.shutdown_receiver_run(
            active.tab_id,
            effect.job_id(),
            active.attribution.instance(),
        ) {
            Ok(true) => {}
            Ok(false) => return false,
            Err(_) => {
                crate::logging::log(format!(
                    "receiver recovery cleanup incomplete job={} boundary=shutdown reason=controller-error",
                    effect.job_id()
                ));
                return false;
            }
        }
        if self
            .cleanup_receiver_instance_files_checked(active.attribution.instance())
            .is_err()
        {
            crate::logging::log(format!(
                "receiver recovery cleanup incomplete job={} boundary=artifacts reason=filesystem-error",
                effect.job_id()
            ));
            return false;
        }
        if effect.action() != ReceiverReconciliationAction::RequeuePreAcceptance {
            let now = self.receiver_now_unix_ms();
            match self
                .services
                .acknowledge_receiver_recovery_cleanup(effect, now)
            {
                Ok(true) => {}
                Ok(false) => return false,
                Err(_) => {
                    crate::logging::log(format!(
                        "receiver recovery cleanup incomplete job={} boundary=acknowledgement reason=store-error",
                        effect.job_id()
                    ));
                    return false;
                }
            }
        }
        self.brain
            .remove_shutdown_receiver_run(
                active.tab_id,
                effect.job_id(),
                active.attribution.instance(),
            )
            .is_some()
    }
}

fn effect_matches_claimed(
    effect: &ReceiverReconciliationEffect,
    claimed: &ClaimedReceiverRun,
) -> bool {
    claimed.claim.job().id() == effect.job_id()
        && claimed.claim.job().token() == effect.token()
        && effect.cleanup_instance() == Some(claimed.remote.instance())
        && effect.cleanup_session_id().is_none()
}
