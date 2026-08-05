//! Receiver listener polling and queued-work dispatch.

use crate::tui::*;

impl App<'_> {
    /// Start the receiver listener only when explicitly requested at TUI
    /// startup. The listener is stored on `App`, so dropping the shell stops
    /// it automatically.
    pub(crate) fn start_receiver_server(&mut self) {
        crate::logging::log("receiver server start requested");
        if self.receiver_server_running() {
            crate::logging::log("receiver server already running");
            return;
        }
        self.receiver_server = None;
        self.receiver_rx = None;
        let (tx, rx) =
            std::sync::mpsc::sync_channel(crate::server::receiver::INBOUND_QUEUE_CAPACITY);
        match crate::server::receiver::ReceiverServer::start(
            &self.command_context,
            crate::server::receiver::DEFAULT_PORT,
            &tx,
        ) {
            Ok(server) => {
                crate::logging::log("receiver server started");
                self.receiver_server = Some(server);
                self.receiver_rx = Some(rx);
                self.flash = Some(FlashKind::Info("receiver server is listening".to_owned()));
            }
            Err(error) => {
                crate::logging::log(format!("receiver server start failed: {error}"));
                self.flash = Some(FlashKind::Error(format!(
                    "receiver server could not start: {error}"
                )));
            }
        }
    }

    /// Drain messages received by the TUI-owned listener. Active agent work is
    /// never interrupted; the queue is consumed when the panel is available.
    pub(crate) fn tick_receiver(&mut self) {
        if self
            .receiver_server
            .as_ref()
            .is_some_and(|server| !server.is_running())
        {
            crate::logging::log("receiver server lost all workers");
            self.receiver_server = None;
            self.receiver_rx = None;
            self.flash = Some(FlashKind::Error(
                "receiver server stopped unexpectedly; restart it to receive messages".to_owned(),
            ));
        }
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
        let control_requests = self
            .receiver_control
            .as_ref()
            .map(crate::tui::singleton::JobSocket::poll)
            .unwrap_or_default();
        for (mut stream, command) in control_requests {
            let response = match command.as_str() {
                "start" => {
                    crate::logging::log("receiver control start");
                    self.start_receiver_server();
                    "receiver server started\n".to_owned()
                }
                "stop" => {
                    crate::logging::log("receiver control stop");
                    self.receiver_server = None;
                    self.receiver_rx = None;
                    "receiver server stopped\n".to_owned()
                }
                "restart" => {
                    crate::logging::log("receiver control restart");
                    self.receiver_server = None;
                    self.receiver_rx = None;
                    self.start_receiver_server();
                    "receiver server restarted\n".to_owned()
                }
                "status" => {
                    crate::logging::log("receiver control status");
                    if self.receiver_server_running() {
                        "receiver server is running\n".to_owned()
                    } else {
                        "receiver server is stopped\n".to_owned()
                    }
                }
                "logs" => {
                    crate::logging::log("receiver control logs");
                    "receiver logs are in the current brain run log\n".to_owned()
                }
                _ => "unknown receiver command\n".to_owned(),
            };
            if let Err(error) = std::io::Write::write_all(&mut stream, response.as_bytes()) {
                crate::logging::log(format!("receiver control response write failed: {error}"));
            }
        }
        if let Some(rx) = &self.receiver_rx {
            for message in rx.try_iter() {
                if message.workspace_id != self.command_context.workspace.id() {
                    crate::logging::log("receiver rejected queued job for another workspace");
                    continue;
                }
                let must_wait = self.brain_turn_active
                    || self.receiver_started.is_some()
                    || !self.receiver_queue.is_empty();
                let modal_open = self.palette.is_some()
                    || self.brain_input.is_some()
                    || self.confirm.is_some()
                    || self.link_picker.is_some()
                    || self.assignee_filter.is_some()
                    || self.help.is_some();
                crate::logging::log(format!(
                    "receiver message queued channel={:?} waiting={} queue_depth={} panel_open={} turn_active={} remote_active={} modal_open={}",
                    message.channel,
                    must_wait,
                    self.receiver_queue.len() + 1,
                    self.brain_panel_open(),
                    self.brain_turn_active,
                    self.receiver_started.is_some(),
                    modal_open
                ));
                if must_wait {
                    match message.channel {
                        crate::server::receiver::Channel::Sms => {
                            let notice = crate::server::reply::processing_notice("sms");
                            crate::server::delivery::send_sms_background(
                                self.command_context.clone(),
                                "queued SMS notice",
                                message.sender.clone(),
                                notice.text,
                            );
                        }
                        crate::server::receiver::Channel::Email => {
                            let recipients = self
                                .receiver_email_recipients(&message.participants, &message.actor);
                            if !recipients.is_empty() {
                                let notice = crate::server::reply::processing_notice("email");
                                let html = crate::server::reply::email_html(&notice.text);
                                crate::server::delivery::send_email_background(
                                    self.command_context.clone(),
                                    "queued email notice",
                                    recipients,
                                    "Brain received your message".to_owned(),
                                    notice.text,
                                    html,
                                );
                            }
                        }
                    }
                }
                self.receiver_queue.push(message);
            }
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
            crate::server::receiver::Channel::Sms => crate::server::reply::sms(&message.body),
            crate::server::receiver::Channel::Email => {
                let _ = crate::server::reply::email_html(&message.body);
                let _ = self.receiver_email_recipients(&message.participants, &message.actor);
                crate::server::reply::email(&message.body)
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
            message.body
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
            self.receiver_sender = Some(message.sender.clone());
            self.receiver_recipients.clone_from(&message.participants);
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
