//! Orderly cleanup for receiver work before generic controller shutdown.

use crate::state::ReceiverJobState;
use crate::state::ReceiverLaunchFailure;
use crate::tui::App;
use crate::tui::receiver::{ActiveReceiverRun, ClaimedReceiverRun, DurableReceiverRun};

use super::diagnostic::receiver_observation_diagnostic;

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
}
