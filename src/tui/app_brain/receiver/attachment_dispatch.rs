//! Non-blocking attachment staging decisions for one claimed receiver run.

use crate::state::ReceiverLaunchFailure;
use crate::tui::App;
use crate::tui::receiver::attachments::{PreparedReceiverAttachments, ReceiverAttachmentEffect};
use crate::tui::receiver::{ClaimedReceiverRun, DurableReceiverRun};

use super::dispatch::{CLAIM_LIFETIME_MS, RETRY_DELAY_MS};

impl App {
    pub(super) fn stage_claimed_receiver_run(&mut self, claimed: ClaimedReceiverRun) {
        match self.services.poll_receiver_attachment_stage(
            claimed.claim.job().id(),
            self.context.command(),
            claimed.claim.job().inbound(),
        ) {
            ReceiverAttachmentEffect::Pending => {
                self.receiver
                    .store_durable_run(DurableReceiverRun::Claimed(claimed));
            }
            ReceiverAttachmentEffect::Ready(attachments) => {
                self.finish_receiver_attachment_stage(claimed, attachments);
            }
            ReceiverAttachmentEffect::Failed => {
                self.finish_receiver_attachment_failure(claimed);
            }
        }
    }

    fn finish_receiver_attachment_stage(
        &mut self,
        claimed: ClaimedReceiverRun,
        attachments: PreparedReceiverAttachments,
    ) {
        let staged_attachment_work = !attachments.staged().is_empty();
        let now = self.receiver_now_unix_ms();
        match self.services.renew_receiver_claim(
            claimed.claim.job().id(),
            claimed.claim.claim().owner(),
            now,
            now.saturating_add(CLAIM_LIFETIME_MS),
        ) {
            Ok(true) if self.receiver.is_enabled() || !staged_attachment_work => {
                self.launch_claimed_receiver_run_with_attachments(claimed, attachments);
            }
            Ok(true) => {
                self.receiver
                    .store_durable_run(DurableReceiverRun::Claimed(claimed));
            }
            Ok(false) => {}
            Err(error) => {
                crate::logging::log(format!(
                    "receiver staged attachment claim validation failed: {error:#}"
                ));
                self.receiver
                    .store_durable_run(DurableReceiverRun::Claimed(claimed));
            }
        }
    }

    fn finish_receiver_attachment_failure(&mut self, claimed: ClaimedReceiverRun) {
        let now = self.receiver_now_unix_ms();
        match self.services.renew_receiver_claim(
            claimed.claim.job().id(),
            claimed.claim.claim().owner(),
            now,
            now.saturating_add(CLAIM_LIFETIME_MS),
        ) {
            Ok(true) => match self.services.record_receiver_launch_retry(
                claimed.claim.job().id(),
                claimed.claim.claim().owner(),
                now,
                now.saturating_add(RETRY_DELAY_MS),
                ReceiverLaunchFailure::Planning,
            ) {
                Ok(Some(_) | None) => {}
                Err(error) => {
                    crate::logging::log(format!(
                        "receiver attachment retry recording failed: {error:#}"
                    ));
                    self.receiver
                        .store_durable_run(DurableReceiverRun::Claimed(claimed));
                }
            },
            Ok(false) => {}
            Err(error) => {
                crate::logging::log(format!(
                    "receiver attachment claim validation failed: {error:#}"
                ));
                self.receiver
                    .store_durable_run(DurableReceiverRun::Claimed(claimed));
            }
        }
    }
}

pub(super) fn localize_attachment_references(
    prompt: &str,
    attachments: &[crate::server::receiver::StagedAttachment],
) -> Option<String> {
    if attachments.is_empty() {
        return Some(prompt.to_owned());
    }
    let marker = "\n\nAttachment references:";
    let start = prompt.rfind(marker)?;
    let mut localized = prompt[..start].to_owned();
    localized.push_str("\n\nLocal attachment files:");
    for attachment in attachments {
        use std::fmt::Write as _;

        let path = attachment.path.as_ref()?;
        let encoded = serde_json::to_string(&path.display().to_string()).ok()?;
        let _ = write!(localized, "\n- path={encoded}");
    }
    Some(localized)
}
