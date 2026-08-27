//! Finite local handoff of one durable terminal unavailable notice.

use crate::tui::App;

const NOTICE_CLAIM_LIFETIME_MS: u64 = 30_000;

impl App {
    pub(super) fn handoff_pending_receiver_notice(&self) {
        let now = self.receiver_now_unix_ms();
        let owner = format!("{}-notice-{}", self.brain.instance(), uuid::Uuid::new_v4());
        let claim = match self.services.claim_receiver_unavailable_notice(
            &owner,
            now,
            now.saturating_add(NOTICE_CLAIM_LIFETIME_MS),
        ) {
            Ok(Some(claim)) => claim,
            Ok(None) => return,
            Err(_) => {
                crate::logging::log("receiver unavailable-notice failed boundary=claim-store");
                return;
            }
        };
        match self
            .services
            .queue_receiver_unavailable_notice(self.context.command(), &claim)
        {
            Ok(true) => {}
            Ok(false) => {
                crate::logging::log("receiver unavailable-notice local handoff was not accepted");
                return;
            }
            Err(_) => {
                crate::logging::log("receiver unavailable-notice failed boundary=local-queue");
                return;
            }
        }
        let acknowledged_at = self.receiver_now_unix_ms();
        match self
            .services
            .acknowledge_receiver_unavailable_notice(&claim, acknowledged_at)
        {
            Ok(true) => {}
            Ok(false) => crate::logging::log(
                "receiver unavailable-notice local handoff acknowledgement lost authority",
            ),
            Err(_) => crate::logging::log(
                "receiver unavailable-notice failed boundary=acknowledgement-store",
            ),
        }
    }
}
