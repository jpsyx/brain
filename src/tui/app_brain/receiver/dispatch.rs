//! Live job-socket polling and queued-work dispatch.

use crate::tui::*;

impl App<'_> {
    /// Drain jobs received on the UUID-local socket. Active agent work is
    /// never interrupted; the queue is consumed when the panel is available.
    pub(crate) fn tick_receiver(&mut self) {
        self.poll_completed_remote_response();
        self.poll_completed_interactive_turn();
        self.maybe_send_processing_delay();
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
            "This is an authenticated {label} message from {} (actor {}). Respond as the user's brain.\n\n{}",
            message.actor.display_name(),
            message.actor.user_id(),
            message.prompt
        );
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
        let launched = self.open_or_focus_brain(Some(&(prompt + &attachments)));
        let _ = crate::tui::receiver_state::commit_dispatch(&mut self.receiver_queue, launched);
        if launched {
            self.receiver_retry_at = None;
            self.receiver_sender = Some(message.authenticated_sender.clone());
            self.receiver_recipients
                .clone_from(&message.allowed_response_recipients);
            self.receiver_response_email
                .clone_from(&message.response_email);
            self.receiver_generation = self.receiver_generation.saturating_add(1);
            self.receiver_started = Some(std::time::Instant::now());
            self.receiver_delay_sent = false;
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
