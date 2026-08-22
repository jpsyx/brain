//! The single seam every outbound email reply passes through.
//!
//! Each reply is addressed by intersecting acceptance-time trusted
//! recipients. That intersection can legitimately come back empty, and an
//! empty recipient list means the user gets nothing at all — the worst
//! failure this channel has, because it is indistinguishable from the agent
//! never finishing. Routing all three delivery sites (processing notice,
//! final response, and the post-teardown fallback) through here keeps that
//! outcome logged instead of silent.

use crate::tui::*;

impl App {
    pub(in crate::tui::app_brain) fn send_email_reply(&self, action: &'static str, message: &str) {
        let target = self.receiver.email_reply_target();
        let recipients = crate::server::delivery::trusted_response_recipients(
            target.response_email.as_deref(),
            &target.recipients,
        );
        if recipients.is_empty() {
            crate::logging::log(format!(
                "receiver email reply dropped action={action} reason=no trusted recipient \
                 (set a response email for this user, or reply from an inbound-allowed address)"
            ));
            return;
        }
        let reply = crate::server::reply::email(message);
        let html = crate::server::reply::email_html(&reply.text);
        crate::server::delivery::send_email_background(
            self.context.command().clone(),
            action,
            recipients,
            crate::server::delivery::reply_subject(target.reply.as_ref()),
            reply.text,
            html,
            target.reply,
        );
    }
}
