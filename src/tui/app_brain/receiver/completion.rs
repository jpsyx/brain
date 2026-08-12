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

    /// Give up on a dispatched turn that never signalled completion.
    ///
    /// Nothing else releases one: the inactivity lease only expires once no
    /// message is in flight, so a wedged turn pinned the panel and every
    /// message behind it waited forever. The sender is told, and the panel is
    /// torn down so the queue can move. The interactive session is restored
    /// only when nothing is waiting, since queued work claims the panel next.
    pub(super) fn abandon_timed_out_remote_turn(&mut self) {
        if !crate::tui::receiver_state::abandons_stalled_turn(
            self.receiver_started,
            self.last_panel_change(),
            std::time::Instant::now(),
        ) {
            return;
        }
        crate::logging::log(format!(
            "receiver turn abandoned: {}s open with no completion and no panel activity for {}s; releasing {} queued message(s)",
            self.receiver_started
                .map_or(0, |started| started.elapsed().as_secs()),
            self.last_panel_change()
                .map_or(0, |changed| changed.elapsed().as_secs()),
            self.receiver_queue.len()
        ));
        // The panel is the only witness to why a turn never finished.
        crate::logging::log(format!(
            "receiver abandoned panel showed: {}",
            self.panel_tail(14).unwrap_or_else(|| "<no panel>".to_owned())
        ));
        let nothing_queued = self.receiver_queue.is_empty();
        self.close_receiver_panel(nothing_queued);
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
                self.send_email_reply("delayed email notice", &notice.text);
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
                self.send_email_reply("final email response", message);
            }
        }
        self.brain_turn_active = false;
        self.receiver_sender = None;
        self.receiver_recipients.clear();
        self.receiver_response_email = None;
        self.receiver_email_reply = None;
        self.receiver_started = None;
        self.receiver_delay_sent = false;
        self.receiver_probe = None;
        self.receiver_panel_activity = None;
        self.receiver_generation = self.receiver_generation.saturating_add(1);
        self.receiver_lease = Some(crate::tui::receiver_state::renew(
            channel,
            self.receiver_generation,
            std::time::Instant::now(),
        ));
        self.reload_after_brain();
    }
}
