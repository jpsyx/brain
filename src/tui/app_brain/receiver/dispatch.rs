//! Live job-socket polling and queued-work dispatch.

use crate::tui::*;

impl App {
    /// Drain jobs received on the UUID-local socket. Active agent work is
    /// never interrupted; the queue is consumed when the panel is available.
    pub(crate) fn tick_receiver(&mut self) {
        self.poll_completed_remote_response();
        self.poll_completed_interactive_turn();
        self.maybe_send_processing_delay();
        // After the completion polls, so a late answer still wins over the
        // deadline it arrived just past.
        self.sample_panel_activity(std::time::Instant::now());
        self.probe_dispatched_receiver_message();
        self.abandon_timed_out_remote_turn();
        if let Some(channel) = self.receiver.warm_lease_expired(std::time::Instant::now()) {
            crate::logging::log(format!(
                "receiver session lease expired channel={channel:?}; restoring interactive session"
            ));
            self.close_receiver_panel(true);
        }
        self.receiver.poll_jobs(self.command_context.workspace.id());
        // Before any gate: a restart is the way out of a queue that is stuck,
        // so it must not be made to wait behind the queue it is clearing.
        self.apply_queued_restart();
        let now = std::time::Instant::now();
        if !self.receiver.retry_ready(now) {
            return;
        }
        if self.receiver.has_pending_work()
            && !self.brain_turn_active
            && !self.receiver.remote_turn_in_flight()
            && !self.receiver_sync_ready()
        {
            return;
        }
        // Only between messages, and only with the panel free: a `/new` that
        // ran mid-turn would cut the conversation in the wrong place and kill
        // the answer someone is already waiting on.
        if !self.brain_turn_active && !self.receiver.remote_turn_in_flight() {
            while self.apply_queued_new_session() {}
        }
        let queued = self.receiver.next_job().cloned();
        let queued_channel = queued.as_ref().map(|message| message.channel);
        let reusable_channel = (queued
            .as_ref()
            .is_some_and(|message| self.session_actor.as_ref() == Some(&message.actor)))
        .then(|| self.receiver.active_channel())
        .flatten();
        match crate::tui::receiver_state::dispatch_action_for_channel(
            queued_channel,
            self.brain_panel_open(),
            reusable_channel,
            self.brain_turn_active,
            self.receiver.remote_turn_in_flight(),
        ) {
            crate::tui::receiver_state::DispatchAction::WaitForTurn => {
                return;
            }
            crate::tui::receiver_state::DispatchAction::CloseIdlePanel => {
                if self.receiver.has_receiver_session() {
                    crate::logging::log("receiver dispatch switching from a warm receiver channel");
                    self.close_receiver_panel(false);
                } else {
                    crate::logging::log("receiver dispatch replacing idle interactive brain panel");
                    self.close_brain();
                }
            }
            crate::tui::receiver_state::DispatchAction::ReuseReceiverPanel
            | crate::tui::receiver_state::DispatchAction::StartNext => {}
        }
        let Some(message) = self.receiver.next_job().cloned() else {
            return;
        };
        let label = match message.channel {
            crate::server::receiver::Channel::Sms => "SMS",
            crate::server::receiver::Channel::Email => "email",
        };
        let _delivery_shape = match message.channel {
            crate::server::receiver::Channel::Sms => crate::server::reply::sms(&message.prompt),
            crate::server::receiver::Channel::Email => {
                let _ = crate::server::reply::email_html(&message.prompt);
                crate::server::reply::email(&message.prompt)
            }
        };
        let _ = crate::server::reply::processing_notice(label);
        let staged = crate::server::receiver::stage_attachments(
            &self.command_context.workspace,
            &self.command_context,
            &message,
        );
        let mut attachments = String::new();
        for attachment in staged {
            use std::fmt::Write;
            let _ = write!(
                attachments,
                "\nAttachment: {}",
                attachment.path.map_or_else(
                    || format!(
                        "{} (unreadable: {})",
                        attachment.source,
                        attachment
                            .error
                            .unwrap_or_else(|| "unknown error".to_owned())
                    ),
                    |path| path.display().to_string(),
                )
            );
        }
        let prompt = format!(
            "This is an authenticated {label} message from {} (actor {}). Respond as the user's brain. If the message asks to add, create, capture, remember, or track a task, create it in Brain's task system; do not perform the task now unless the sender explicitly asks you to.\n\n{}",
            message.actor.display_name(),
            message.actor.user_id(),
            message.prompt
        );
        // A `/new` on this channel makes the launch that follows it refuse to
        // resume, which is what retires the old conversation.
        self.receiver.prepare_channel_launch(message.channel);
        let reusing_receiver_panel =
            self.receiver.has_receiver_session() && self.brain_panel_open();
        if reusing_receiver_panel {
            if let Some(session_id) = self.receiver.receiver_response_id() {
                let response_path = self
                    .command_context
                    .workspace
                    .paths()
                    .responses_dir()
                    .join(format!("{session_id}.json"));
                let _ = std::fs::remove_file(response_path);
            }
        } else {
            self.receiver.request_receiver_launch(message.actor.clone());
        }
        // Which delivery a message took is the first thing to know when one
        // goes unanswered: a fresh launch passes the prompt as a command
        // argument, while a reuse types it into a live composer.
        crate::logging::log(format!(
            "receiver dispatch delivering channel={:?} via {}",
            message.channel,
            if reusing_receiver_panel {
                "warm-panel injection"
            } else {
                "fresh launch argument"
            }
        ));
        if reusing_receiver_panel {
            // What the composer already showed explains a prompt that lands but
            // never submits: leftover text, or something waiting on a keypress.
            crate::logging::log(format!(
                "receiver panel before injection: {}",
                self.panel_tail(14)
                    .unwrap_or_else(|| "<no panel>".to_owned())
            ));
        }
        let launched = self.open_or_focus_brain(Some(&(prompt + &attachments)));
        let dispatched_at = std::time::Instant::now();
        let _ = self
            .receiver
            .finish_dispatch(launched, &message, dispatched_at);
        if launched {
            crate::logging::log(format!(
                "receiver dispatch started channel={:?} queue_depth={}",
                message.channel,
                self.receiver.pending_count()
            ));
        } else {
            crate::logging::log(format!(
                "receiver dispatch launch failed; message retained channel={:?} queue_depth={}",
                message.channel,
                self.receiver.pending_count()
            ));
        }
    }
}
