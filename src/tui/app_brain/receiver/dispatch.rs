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
        if let Some(lease) = self.receiver_lease
            && crate::tui::receiver_state::expired(
                lease,
                std::time::Instant::now(),
                self.receiver_lease.map(|current| current.channel),
                self.receiver_generation,
            )
            && self.receiver_session_id.is_some()
            && self.receiver_started.is_none()
        {
            crate::logging::log(format!(
                "receiver session lease expired channel={:?}; restoring interactive session",
                lease.channel
            ));
            self.close_receiver_panel(true);
        }
        if let Some(socket) = self.receiver_control.as_ref() {
            socket.poll_jobs(
                self.command_context.workspace.id(),
                &mut self.receiver_queue,
            );
        }
        // Before any gate: a restart is the way out of a queue that is stuck,
        // so it must not be made to wait behind the queue it is clearing.
        self.apply_queued_restart();
        let now = std::time::Instant::now();
        if !crate::tui::receiver_state::retry_ready(self.receiver_retry_at, now) {
            return;
        }
        self.receiver_retry_at = None;
        if !self.receiver_queue.is_empty()
            && !self.brain_turn_active
            && self.receiver_started.is_none()
            && !self.receiver_sync_ready()
        {
            return;
        }
        // Only between messages, and only with the panel free: a `/new` that
        // ran mid-turn would cut the conversation in the wrong place and kill
        // the answer someone is already waiting on.
        if !self.brain_turn_active && self.receiver_started.is_none() {
            while self.apply_queued_new_session() {}
        }
        let queued = self.receiver_queue.first();
        let queued_channel = queued.map(|message| message.channel);
        let reusable_channel = self.receiver_lease.and_then(|lease| {
            (queued.is_some_and(|message| self.session_actor.as_ref() == Some(&message.actor)))
                .then_some(lease.channel)
        });
        match crate::tui::receiver_state::dispatch_action_for_channel(
            queued_channel,
            self.brain_panel_open(),
            reusable_channel,
            self.brain_turn_active,
            self.receiver_started.is_some(),
        ) {
            crate::tui::receiver_state::DispatchAction::WaitForTurn => {
                return;
            }
            crate::tui::receiver_state::DispatchAction::CloseIdlePanel => {
                if self.receiver_session_id.is_some() {
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
        let message = self.receiver_queue[0].clone();
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
        self.receiver_force_fresh = self.receiver_new_session.remove(&message.channel);
        let reusing_receiver_panel = self.receiver_session_id.is_some() && self.brain_panel_open();
        if reusing_receiver_panel {
            if let Some(session_id) = self.receiver_session_id.as_deref() {
                let response_path = self
                    .command_context
                    .workspace
                    .paths()
                    .responses_dir()
                    .join(format!("{session_id}.json"));
                let _ = std::fs::remove_file(response_path);
            }
        } else {
            self.requested_receiver_actor = Some(message.actor.clone());
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
        let _ = crate::tui::receiver_state::commit_dispatch(&mut self.receiver_queue, launched);
        if launched {
            self.receiver_retry_at = None;
            self.receiver_sender = Some(message.authenticated_sender.clone());
            self.receiver_recipients
                .clone_from(&message.allowed_response_recipients);
            self.receiver_response_email
                .clone_from(&message.response_email);
            self.receiver_email_reply.clone_from(&message.email_reply);
            self.receiver_generation = self.receiver_generation.saturating_add(1);
            let dispatched_at = std::time::Instant::now();
            self.receiver_started = Some(dispatched_at);
            self.receiver_delay_sent = false;
            self.schedule_receiver_probes(dispatched_at);
            self.receiver_lease = Some(crate::tui::receiver_state::renew(
                message.channel,
                self.receiver_generation,
                std::time::Instant::now(),
            ));
            crate::logging::log(format!(
                "receiver dispatch started channel={:?} queue_depth={}",
                message.channel,
                self.receiver_queue.len()
            ));
        } else {
            self.requested_receiver_actor = None;
            self.receiver_retry_at =
                Some(std::time::Instant::now() + std::time::Duration::from_secs(5));
            crate::logging::log(format!(
                "receiver dispatch launch failed; message retained channel={:?} queue_depth={}",
                message.channel,
                self.receiver_queue.len()
            ));
        }
    }
}
