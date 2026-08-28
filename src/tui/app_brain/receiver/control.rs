//! Durable messages that steer receiver session continuity.

use crate::tui::App;
use crate::tui::receiver::ClaimedReceiverRun;

impl App {
    pub(super) fn apply_receiver_restarts(&self) {
        loop {
            let now = self.receiver_now_unix_ms();
            match self.services.apply_next_receiver_restart(now) {
                Ok(Some(_)) => {}
                Ok(None) => return,
                Err(error) => {
                    crate::logging::log(format!(
                        "durable receiver restart control failed: {error:#}"
                    ));
                    return;
                }
            }
        }
    }

    pub(super) fn complete_receiver_new_session(&mut self, claimed: ClaimedReceiverRun) {
        let job = claimed.claim.job();
        let now = self.receiver_now_unix_ms();
        match self.services.complete_receiver_new_session(
            job.id(),
            claimed.claim.claim().owner(),
            now,
        ) {
            Ok(true) => {
                if self.receiver.is_enabled() {
                    self.claim_receiver_run();
                }
            }
            Ok(false) => {}
            Err(error) => {
                crate::logging::log(format!(
                    "durable receiver new-session control failed: {error:#}"
                ));
                self.receiver
                    .store_durable_run(crate::tui::receiver::DurableReceiverRun::Claimed(claimed));
            }
        }
    }
}
