//! Messages that steer the receiver instead of asking it something.
//!
//! Two commands, deliberately handled at different moments. `/restart` is a
//! rescue: its whole value is that it takes effect the instant it arrives, so
//! it is applied as soon as it is polled off the socket, even while an answer
//! is in flight. `/new` is a conversational boundary: its whole value is
//! *where* it falls, so it waits its turn in the queue and is applied only
//! between messages.

use crate::server::receiver::InboundJob;
use crate::tui::App;

impl App {
    /// Apply a queued `/restart` the moment it is seen.
    ///
    /// Deliberately not gated on the panel being free. A sender restarts
    /// *because* they are stuck behind something, so making the command wait
    /// for the thing it is meant to escape would leave it useless.
    pub(crate) fn apply_receiver_restart(
        &self,
        plan: &crate::server::receiver::RestartPlan<InboundJob>,
    ) {
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
    /// The tick coordinator re-enters this stage after the effect, so adjacent
    /// controls are consumed before ordinary work can dispatch.
    pub(crate) fn apply_receiver_new_session(&mut self, job: &InboundJob) {
        crate::logging::log(format!(
            "receiver control /new for channel={:?}; next message opens a fresh session",
            job.channel
        ));
        // A warm panel would be reused for the next message, which is exactly
        // the conversation the sender asked to leave, so it is retired now.
        if self.receiver.has_receiver_session()
            && self.receiver.active_channel() == Some(job.channel)
        {
            self.close_receiver_panel(false);
        }
        self.reply_to_job(
            job,
            "new session acknowledgement",
            &crate::server::reply::new_session_notice(channel_label(job.channel)).text,
        );
    }
}

const fn channel_label(channel: crate::server::receiver::Channel) -> &'static str {
    match channel {
        crate::server::receiver::Channel::Sms => "sms",
        crate::server::receiver::Channel::Email => "email",
    }
}
