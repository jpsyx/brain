//! Durable unavailable-notice delivery facade.

use anyhow::Result;

use super::AppServices;
use crate::workspace::CommandContext;

pub(crate) trait ReceiverNoticeDelivery: Send {
    fn queue(
        &self,
        command: &CommandContext,
        inbound: &crate::server::receiver::InboundJob,
        message: &str,
    ) -> Result<bool>;
}

pub(super) struct SystemReceiverNoticeDelivery;

impl ReceiverNoticeDelivery for SystemReceiverNoticeDelivery {
    fn queue(
        &self,
        command: &CommandContext,
        inbound: &crate::server::receiver::InboundJob,
        message: &str,
    ) -> Result<bool> {
        const ACTION: &str = "receiver unavailable notice";
        match inbound.channel {
            crate::server::receiver::Channel::Sms => {
                crate::server::delivery::queue_sms_background(
                    command.clone(),
                    ACTION,
                    inbound.authenticated_sender.clone(),
                    crate::server::reply::sms(message).text,
                )?;
            }
            crate::server::receiver::Channel::Email => {
                let recipients = crate::server::delivery::trusted_response_recipients(
                    inbound.response_email.as_deref(),
                    &inbound.allowed_response_recipients,
                );
                if recipients.is_empty() {
                    return Ok(false);
                }
                let reply = crate::server::reply::email(message);
                let html = crate::server::reply::email_html(&reply.text);
                crate::server::delivery::queue_email_background(
                    command.clone(),
                    ACTION,
                    recipients,
                    crate::server::delivery::reply_subject(inbound.email_reply.as_ref()),
                    reply.text,
                    html,
                    inbound.email_reply.clone(),
                )?;
            }
        }
        Ok(true)
    }
}

impl AppServices {
    pub(crate) fn claim_receiver_unavailable_notice(
        &self,
        owner: &str,
        now_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Option<crate::state::ReceiverUnavailableNoticeClaim>> {
        self.db
            .claim_next_receiver_unavailable_notice(owner, now_unix_ms, expires_at_unix_ms)
    }

    pub(crate) fn queue_receiver_unavailable_notice(
        &self,
        command: &CommandContext,
        claim: &crate::state::ReceiverUnavailableNoticeClaim,
    ) -> Result<bool> {
        self.receiver_notice_delivery.queue(
            command,
            claim.inbound(),
            crate::server::receiver::unavailable_message(),
        )
    }

    pub(crate) fn acknowledge_receiver_unavailable_notice(
        &self,
        claim: &crate::state::ReceiverUnavailableNoticeClaim,
        now_unix_ms: u64,
    ) -> Result<bool> {
        self.db.acknowledge_receiver_unavailable_notice(
            claim.job_id(),
            claim.token(),
            claim.owner(),
            now_unix_ms,
        )
    }

    #[cfg(test)]
    pub(crate) fn replace_receiver_notice_delivery(
        &mut self,
        delivery: Box<dyn ReceiverNoticeDelivery>,
    ) {
        self.receiver_notice_delivery = delivery;
    }
}
