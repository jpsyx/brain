//! Interactive and remote completion polling plus provider delivery.

use crate::tui::*;

impl App<'_> {
    /// A Stop hook marks the end of an interactive turn without killing the
    /// persistent panel. If remote work is waiting, close only after that
    /// completion signal so the active turn is never interrupted.
    pub(super) fn poll_completed_interactive_turn(&mut self) {
        if !crate::tui::receiver_state::should_poll_interactive_completion(
            self.brain_turn_active,
            self.receiver_started.is_some(),
        ) {
            return;
        }
        let Some(session_id) = self.interactive_session_id.clone() else {
            return;
        };
        let path = self
            .command_context
            .workspace
            .paths()
            .responses_dir()
            .join(format!("{session_id}.json"));
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
            return;
        };
        if !crate::server::reply::completion_matches_actor(&value, &self.interactive_actor) {
            let _ = std::fs::remove_file(path);
            crate::logging::log("interactive completion actor mismatch discarded");
            return;
        }
        let _ = std::fs::remove_file(path);
        self.brain_turn_active = false;
        crate::logging::log("interactive brain turn completed");
        if !self.receiver_queue.is_empty() {
            crate::logging::log("interactive turn complete; switching to queued receiver work");
            self.close_brain();
        }
    }

    pub(super) fn maybe_send_processing_delay(&mut self) {
        if self.receiver_delay_sent
            || self
                .receiver_started
                .is_none_or(|started| started.elapsed() < std::time::Duration::from_secs(120))
        {
            return;
        }
        let (Some(channel), Some(sender)) = (
            self.receiver_lease.map(|lease| lease.channel),
            self.receiver_sender.clone(),
        ) else {
            return;
        };
        let notice = crate::server::reply::processing_notice(match channel {
            crate::server::receiver::Channel::Sms => "sms",
            crate::server::receiver::Channel::Email => "email",
        });
        match channel {
            crate::server::receiver::Channel::Sms => {
                crate::server::delivery::send_sms_background(
                    self.command_context.clone(),
                    "delayed SMS notice",
                    sender,
                    notice.text,
                );
            }
            crate::server::receiver::Channel::Email => {
                let recipients = self.session_actor.as_ref().map_or_else(Vec::new, |actor| {
                    self.receiver_email_recipients(&self.receiver_recipients, actor)
                });
                if !recipients.is_empty() {
                    let html = crate::server::reply::email_html(&notice.text);
                    crate::server::delivery::send_email_background(
                        self.command_context.clone(),
                        "delayed email notice",
                        recipients,
                        "Brain is still working".to_owned(),
                        notice.text,
                        html,
                    );
                }
            }
        }
        self.receiver_delay_sent = true;
    }

    pub(super) fn poll_completed_remote_response(&mut self) {
        let (Some(session_id), Some(channel), Some(sender)) = (
            self.receiver_session_id.clone(),
            self.receiver_lease.map(|lease| lease.channel),
            self.receiver_sender.clone(),
        ) else {
            return;
        };
        let path = self
            .command_context
            .workspace
            .paths()
            .responses_dir()
            .join(format!("{session_id}.json"));
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
            return;
        };
        if self
            .session_actor
            .as_ref()
            .is_none_or(|actor| !crate::server::reply::completion_matches_actor(&value, actor))
        {
            let _ = std::fs::remove_file(path);
            crate::logging::log("receiver completion actor mismatch discarded");
            self.close_receiver_panel(true);
            return;
        }
        let Some(message) = value.get("message").and_then(serde_json::Value::as_str) else {
            return;
        };
        let _ = std::fs::remove_file(path);
        crate::logging::log(format!(
            "receiver agent response completed channel={channel:?}"
        ));
        match channel {
            crate::server::receiver::Channel::Sms => {
                let reply = crate::server::reply::sms(message);
                crate::server::delivery::send_sms_background(
                    self.command_context.clone(),
                    "final SMS response",
                    sender,
                    reply.text,
                );
            }
            crate::server::receiver::Channel::Email => {
                let recipients = self.session_actor.as_ref().map_or_else(Vec::new, |actor| {
                    self.receiver_email_recipients(&self.receiver_recipients, actor)
                });
                if !recipients.is_empty() {
                    let reply = crate::server::reply::email(message);
                    let html = crate::server::reply::email_html(&reply.text);
                    crate::server::delivery::send_email_background(
                        self.command_context.clone(),
                        "final email response",
                        recipients,
                        "Brain response".to_owned(),
                        reply.text,
                        html,
                    );
                }
            }
        }
        self.brain_turn_active = false;
        self.receiver_sender = None;
        self.receiver_recipients.clear();
        self.receiver_started = None;
        self.receiver_delay_sent = false;
        self.receiver_generation = self.receiver_generation.saturating_add(1);
        self.receiver_lease = Some(crate::tui::receiver_state::renew(
            channel,
            self.receiver_generation,
            std::time::Instant::now(),
        ));
        self.reload_after_brain();
    }

    pub(in crate::tui::app_brain) fn receiver_email_recipients(
        &self,
        participants: &[String],
        actor: &crate::actor::ActorContext,
    ) -> Vec<String> {
        let Ok(users) = crate::users::UsersStore::load(&self.command_context.workspace) else {
            return Vec::new();
        };
        let receiving =
            crate::env::get(&self.command_context, "resend_from_email").unwrap_or_default();
        crate::server::delivery::actor_thread_recipients(participants, &users, actor, &receiving)
    }
}
