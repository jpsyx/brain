//! Completion delivery using one immutable accepted receiver job.

use crate::server::receiver::InboundJob;
use crate::tui::App;

impl App {
    pub(in crate::tui::app_brain) fn reply_to_job(
        &self,
        job: &InboundJob,
        action: &'static str,
        message: &str,
    ) {
        match job.channel {
            crate::server::receiver::Channel::Sms => {
                crate::server::delivery::send_sms_background(
                    self.context.command().clone(),
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
                        "receiver reply dropped action={action} reason=no trusted recipient"
                    ));
                    return;
                }
                let reply = crate::server::reply::email(message);
                let html = crate::server::reply::email_html(&reply.text);
                crate::server::delivery::send_email_background(
                    self.context.command().clone(),
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
}
