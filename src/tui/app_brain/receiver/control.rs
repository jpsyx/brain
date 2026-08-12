//! Messages that steer the receiver instead of asking it something.
//!
//! Two commands, deliberately handled at different moments. `/restart` is a
//! rescue: its whole value is that it takes effect the instant it arrives, so
//! it is applied as soon as it is polled off the socket, even while an answer
//! is in flight. `/new` is a conversational boundary: its whole value is
//! *where* it falls, so it waits its turn in the queue and is applied only
//! between messages.

use crate::server::receiver::{ControlCommand, InboundJob, parse_control_command};
use crate::tui::*;

impl App<'_> {
    /// Reply to one specific job, rather than to whatever is in flight.
    ///
    /// The receiver's usual reply path answers the message currently being
    /// worked on, using the sender and thread recorded on `App`. A dropped or
    /// refused job is not that message, so its own recipients and thread come
    /// off the job itself; using the live ones would send someone else's
    /// message to the wrong person.
    pub(in crate::tui::app_brain) fn reply_to_job(
        &self,
        job: &InboundJob,
        action: &'static str,
        message: &str,
    ) {
        match job.channel {
            crate::server::receiver::Channel::Sms => {
                crate::server::delivery::send_sms_background(
                    self.command_context.clone(),
                    action,
                    job.authenticated_sender.clone(),
                    crate::server::reply::sms(message).text,
                );
            }
            crate::server::receiver::Channel::Email => {
                let recipients = crate::server::delivery::trusted_response_recipients(
                    job.response_email.as_deref(),
                    &job.allowed_response_recipients,
                );
                if recipients.is_empty() {
                    crate::logging::log(format!(
                        "receiver control reply dropped action={action} reason=no trusted recipient"
                    ));
                    return;
                }
                let reply = crate::server::reply::email(message);
                let html = crate::server::reply::email_html(&reply.text);
                crate::server::delivery::send_email_background(
                    self.command_context.clone(),
                    action,
                    recipients,
                    crate::server::delivery::reply_subject(job.email_reply.as_ref()),
                    reply.text,
                    html,
                    job.email_reply.clone(),
                );
            }
        }
    }

    /// Apply a queued `/restart` the moment it is seen.
    ///
    /// Deliberately not gated on the panel being free. A sender restarts
    /// *because* they are stuck behind something, so making the command wait
    /// for the thing it is meant to escape would leave it useless.
    pub(crate) fn apply_queued_restart(&mut self) {
        let Some(plan) = crate::server::receiver::take_restart(&mut self.receiver_queue, |job| {
            parse_control_command(&job.prompt) == Some(ControlCommand::Restart)
        }) else {
            return;
        };
        crate::logging::log(format!(
            "receiver control /restart from channel={:?}; dropping {} queued message(s)",
            plan.command.channel,
            plan.dropped.len()
        ));
        for job in &plan.dropped {
            self.reply_to_job(
                job,
                "dropped by restart",
                &crate::server::reply::unanswered_notice(channel_label(job.channel)).text,
            );
        }
        self.reply_to_job(
            &plan.command,
            "restart acknowledgement",
            &crate::server::reply::restart_notice(
                channel_label(plan.command.channel),
                plan.dropped.len(),
            )
            .text,
        );
    }

    /// Apply a `/new` sitting at the head of the queue, if the panel is free.
    ///
    /// Returns `true` when one was consumed, so the caller can re-enter rather
    /// than dispatch a command as if it were a prompt.
    pub(crate) fn apply_queued_new_session(&mut self) -> bool {
        let Some(job) = self.receiver_queue.first() else {
            return false;
        };
        if parse_control_command(&job.prompt) != Some(ControlCommand::NewSession) {
            return false;
        }
        let job = self.receiver_queue.remove(0);
        self.receiver_new_session.insert(job.channel);
        crate::logging::log(format!(
            "receiver control /new for channel={:?}; next message opens a fresh session",
            job.channel
        ));
        // A warm panel would be reused for the next message, which is exactly
        // the conversation the sender asked to leave, so it is retired now.
        if self.receiver_session_id.is_some()
            && self.receiver_lease.map(|lease| lease.channel) == Some(job.channel)
        {
            self.close_receiver_panel(false);
        }
        self.reply_to_job(
            &job,
            "new session acknowledgement",
            &crate::server::reply::new_session_notice(channel_label(job.channel)).text,
        );
        true
    }
}

const fn channel_label(channel: crate::server::receiver::Channel) -> &'static str {
    match channel {
        crate::server::receiver::Channel::Sms => "sms",
        crate::server::receiver::Channel::Email => "email",
    }
}
