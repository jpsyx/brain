//! Orderly cleanup for receiver work before generic controller shutdown.

use crate::state::ReceiverLaunchFailure;
use crate::tui::App;
use crate::tui::receiver::{ActiveReceiverRun, ClaimedReceiverRun, DurableReceiverRun};

impl App {
    pub(crate) fn shutdown_receiver_runtime(&mut self) {
        match self.receiver.take_durable_run() {
            DurableReceiverRun::Idle => self.services.shutdown_receiver_attachments(),
            DurableReceiverRun::Claimed(claimed) => self.shutdown_claimed_receiver_run(&claimed),
            DurableReceiverRun::Active(active) => self.shutdown_active_receiver_run(active),
        }
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
        let owned = match self.authorize_receiver_owner_now(&claim) {
            Ok(Some(_)) => true,
            Ok(None) => false,
            Err(error) => {
                crate::logging::log(format!(
                    "receiver shutdown ownership check failed: {error:#}"
                ));
                false
            }
        };
        if owned {
            if let Err(error) = self.services.release_receiver_session(&attribution) {
                crate::logging::log(format!(
                    "receiver session shutdown cleanup failed: {error:#}"
                ));
            }
        }
        let removed = self.brain.remove_receiver_run(tab_id);
        if removed.as_ref().is_some_and(|removed| {
            removed.job_id != claim.job().id() || removed.instance != attribution.instance()
        }) {
            crate::logging::log("receiver tab identity changed before shutdown cleanup");
        }
        let _ = std::fs::remove_file(
            self.context
                .workspace()
                .paths()
                .responses_dir()
                .join(format!("{}.json", attribution.instance())),
        );
        drop(attachments);
        if owned {
            match self.retry_receiver_owner_now(&claim, ReceiverLaunchFailure::Spawn) {
                Ok(Some(_)) => {}
                Ok(None) => crate::logging::log(
                    "receiver shutdown retry lost durable ownership during cleanup",
                ),
                Err(error) => crate::logging::log(format!(
                    "receiver shutdown retry recording failed: {error:#}"
                )),
            }
        }
    }
}
