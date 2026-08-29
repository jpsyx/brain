//! Selection of one due persisted recovery for same-session launch.

use crate::tui::App;
use crate::tui::receiver::{ClaimedReceiverRun, ReceiverRunIdentity};

impl App {
    pub(in crate::tui::app_brain::receiver) fn claim_receiver_recovery_run(&mut self) -> bool {
        if !self.brain.receiver_run_observations().is_empty() {
            return true;
        }
        let identity = ReceiverRunIdentity::new(self.brain.instance());
        let now = self.receiver_now_unix_ms();
        match self.services.claim_receiver_recovery_run(
            identity.instance(),
            now,
            now.saturating_add(super::super::dispatch::CLAIM_LIFETIME_MS),
        ) {
            Ok(Some(claim)) => {
                self.launch_claimed_receiver_recovery(ClaimedReceiverRun {
                    claim,
                    identity,
                    freshness_ready: true,
                });
                true
            }
            Ok(None) => false,
            Err(_) => {
                crate::logging::log("receiver recovery failed boundary=claim-store");
                true
            }
        }
    }
}
