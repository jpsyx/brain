//! Interactive and remote completion polling plus provider delivery.

use crate::tui::*;

impl App {
    /// A Stop hook marks the end of an interactive turn without killing the
    /// persistent panel. If remote work is waiting, close only after that
    /// completion signal so the active turn is never interrupted.
    pub(super) fn poll_completed_interactive_turn(&mut self, session_id: &str) {
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
        if self.receiver.has_pending_work() {
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
        crate::logging::log(format!(
            "receiver turn abandoned: {}s open with no completion and no panel activity for {}s; releasing {} queued message(s)",
            self.receiver
                .remote_started_at()
                .map_or(0, |started| started.elapsed().as_secs()),
            self.last_panel_change()
                .map_or(0, |changed| changed.elapsed().as_secs()),
            self.receiver.pending_count()
        ));
        // The panel is the only witness to why a turn never finished.
        crate::logging::log(format!(
            "receiver abandoned panel showed: {}",
            self.panel_tail(14)
                .unwrap_or_else(|| "<no panel>".to_owned())
        ));
        let nothing_queued = !self.receiver.has_pending_work();
        self.close_receiver_panel(nothing_queued);
    }

    pub(super) fn send_processing_delay(&self, target: crate::tui::receiver::DeliveryTarget) {
        let channel = target.channel;
        let notice = crate::server::reply::processing_notice(match channel {
            crate::server::receiver::Channel::Sms => "sms",
            crate::server::receiver::Channel::Email => "email",
        });
        match channel {
            crate::server::receiver::Channel::Sms => {
                crate::server::delivery::send_sms_background(
                    self.command_context.clone(),
                    "delayed SMS notice",
                    target.sender,
                    notice.text,
                );
            }
            crate::server::receiver::Channel::Email => {
                self.send_email_reply("delayed email notice", &notice.text);
            }
        }
    }

    pub(super) fn poll_completed_remote_response(
        &mut self,
        target: crate::tui::receiver::RemoteCompletionTarget,
    ) {
        let session_id = target.response_id;
        let channel = target.channel;
        let sender = target.sender;
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
        if crate::sync::config::SyncConfig::load(&self.command_context).is_configured() {
            let _ = self.receiver_sync_runtime.spawn_detached_sync(
                &self.command_context.workspace,
                crate::sync::args::Direction::Push,
            );
        }
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
        self.receiver
            .finish_remote_response(std::time::Instant::now());
        self.reload_after_brain();
    }
}
