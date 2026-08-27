//! Durable receiver-run coordination from the application event loop.

use crate::tui::App;
use crate::tui::receiver::{ClaimedReceiverRun, DurableReceiverRun, ReceiverRemoteSession};

pub(super) const CLAIM_LIFETIME_MS: u64 = 30_000;
pub(super) const RETRY_DELAY_MS: u64 = 5_000;

impl App {
    /// Advance the single durable receiver consumer by one non-blocking step.
    pub(crate) fn tick_receiver(&mut self) {
        let receiver_enabled = self.receiver.is_enabled();
        if receiver_enabled {
            self.reconcile_receiver_job();
            self.handoff_pending_receiver_notice();
            self.apply_receiver_restarts();
            #[cfg(test)]
            self.receiver.run_after_restart_scan_hook();
        }
        let run = match self.receiver.take_durable_run() {
            DurableReceiverRun::AnswerCleanupPending(active) => {
                self.continue_receiver_answer_controller_cleanup(active);
                return;
            }
            run => run,
        };
        self.continue_oldest_receiver_answer_cleanup();
        match run {
            DurableReceiverRun::Active(active) => self.tick_active_receiver_run(active),
            DurableReceiverRun::AnswerCleanupPending(_) => unreachable!(),
            DurableReceiverRun::Claimed(claimed) => self.continue_claimed_receiver_run(claimed),
            DurableReceiverRun::RecoveryClaimed(claimed) if receiver_enabled => {
                self.launch_claimed_receiver_recovery(claimed);
            }
            DurableReceiverRun::RecoveryClaimed(claimed) => {
                self.hold_claimed_receiver_recovery(claimed);
            }
            DurableReceiverRun::RecoveryPreSpawnCleanup(cleanup) => {
                super::recovery_launch::pre_spawn_cleanup::continue_recovery_pre_spawn_cleanup(
                    &mut self.receiver,
                    &self.services,
                    cleanup,
                );
            }
            DurableReceiverRun::RecoverySpawned(spawned) => {
                self.continue_spawned_receiver_recovery(spawned);
            }
            DurableReceiverRun::CleanupPending(mut pending) => {
                if pending.defer_once {
                    pending.defer_once = false;
                    self.receiver
                        .store_durable_run(DurableReceiverRun::CleanupPending(pending));
                } else {
                    self.continue_receiver_cleanup(pending);
                }
            }
            DurableReceiverRun::Idle if receiver_enabled => {
                if !self.claim_receiver_recovery_run() {
                    self.claim_receiver_run();
                }
            }
            DurableReceiverRun::Idle => {}
        }
    }

    pub(super) fn claim_receiver_run(&mut self) {
        if !self.brain.receiver_run_observations().is_empty() {
            return;
        }
        let remote = ReceiverRemoteSession::new(self.brain.instance());
        let now = self.receiver_now_unix_ms();
        match self.services.claim_receiver_run(
            remote.instance(),
            now,
            now.saturating_add(CLAIM_LIFETIME_MS),
        ) {
            Ok(Some(claim)) => {
                self.continue_claimed_receiver_run(ClaimedReceiverRun {
                    claim,
                    remote,
                    freshness_ready: false,
                });
            }
            Ok(None) => {}
            Err(error) => crate::logging::log(format!("durable receiver claim failed: {error:#}")),
        }
    }

    fn continue_claimed_receiver_run(&mut self, mut claimed: ClaimedReceiverRun) {
        let now = self.receiver_now_unix_ms();
        match self.services.renew_receiver_claim(
            claimed.claim.job().id(),
            claimed.claim.claim().owner(),
            now,
            now.saturating_add(CLAIM_LIFETIME_MS),
        ) {
            Ok(true) => {}
            Ok(false) => {
                self.services
                    .cancel_receiver_attachment_stage(claimed.claim.job().id());
                return;
            }
            Err(error) => {
                crate::logging::log(format!("receiver pending claim renewal failed: {error:#}"));
                self.receiver
                    .store_durable_run(DurableReceiverRun::Claimed(claimed));
                return;
            }
        }
        if !claimed.freshness_ready {
            if self.execute_receiver_sync_freshness_effect()
                == crate::tui::receiver::ReceiverEffectOutcome::FreshnessPending
            {
                self.receiver
                    .store_durable_run(DurableReceiverRun::Claimed(claimed));
                return;
            }
            claimed.freshness_ready = true;
        }
        if crate::server::receiver::parse_control_command(&claimed.claim.job().inbound().prompt)
            == Some(crate::server::receiver::ControlCommand::NewSession)
        {
            self.complete_receiver_new_session(claimed);
            return;
        }
        if !self.receiver.is_enabled() {
            self.services
                .cancel_receiver_attachment_stage(claimed.claim.job().id());
            self.receiver
                .store_durable_run(DurableReceiverRun::Claimed(claimed));
            return;
        }
        self.stage_claimed_receiver_run(claimed);
    }

    fn hold_claimed_receiver_recovery(&mut self, claimed: ClaimedReceiverRun) {
        let now = self.receiver_now_unix_ms();
        match self.services.renew_receiver_claim(
            claimed.claim.job().id(),
            claimed.claim.claim().owner(),
            now,
            now.saturating_add(CLAIM_LIFETIME_MS),
        ) {
            Ok(true) | Err(_) => self
                .receiver
                .store_durable_run(DurableReceiverRun::RecoveryClaimed(claimed)),
            Ok(false) => {}
        }
    }

    pub(super) fn receiver_now_unix_ms(&self) -> u64 {
        u64::try_from(self.services.utc_now().timestamp_millis()).unwrap_or(0)
    }
}
