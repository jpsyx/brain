//! `App` brain-panel lifecycle: open/resume, close, focus, and seeding the
//! session with a prefilled prompt.
//!
//! The brain panel is a persistent, resumable `claude` session (the same
//! model as the `brain` sibling shell): opening it resumes the
//! most-recently-active free session for this shell (lock + recency), or
//! starts a fresh one; closing it ends that claude process but leaves the
//! conversation resumable next time. There is no completion view and no
//! queue — the panel simply stays open until the user closes it (Ctrl+X /
//! "Close brain") or claude exits.

use super::*;

use std::path::{Path, PathBuf};

use crate::pty_pane::PtyPane;
use crate::session::{self, AgentKind, Plan};

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
            .map(crate::server::receiver::ControlSocket::poll)
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

    /// A Stop hook marks the end of an interactive turn without killing the
    /// persistent panel. If remote work is waiting, close only after that
    /// completion signal so the active turn is never interrupted.
    fn poll_completed_interactive_turn(&mut self) {
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

    fn maybe_send_processing_delay(&mut self) {
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

    fn poll_completed_remote_response(&mut self) {
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
        self.pending_brain_submit = 0;
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

    /// Whether the brain panel is on screen (a live agent PTY).
    pub(crate) fn brain_panel_open(&self) -> bool {
        self.brain.is_some()
    }

    pub(crate) fn receiver_server_running(&self) -> bool {
        self.receiver_server
            .as_ref()
            .is_some_and(crate::server::receiver::ReceiverServer::is_running)
    }

    fn receiver_email_recipients(
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

    pub(crate) fn focus_brain(&mut self) {
        if self.any_brain_panel_visible() {
            self.alert = None;
            self.focus = Panel::Brain;
        }
    }

    /// Switch focus to the tasks panel. On a Brain → Tasks transition we
    /// also reload tasks.csv + habits.csv: brain-driven actions (defer,
    /// remove, complete) mutate the CSVs asynchronously and we have no
    /// completion signal, so the focus switch is our cue to pick up
    /// whatever changed while the user was over in the brain panel.
    pub(crate) fn focus_tasks(&mut self) {
        let was_on_brain = self.focus == Panel::Brain;
        self.alert = None;
        self.focus = Panel::Tasks;
        if was_on_brain {
            self.reload_after_brain();
        }
    }

    /// Open the brain panel (or focus it if already open). Resume the
    /// most-recently-active free session whose transcript still exists on
    /// disk and lock it; otherwise start a fresh session with a tasks-chosen
    /// id. When `prompt` is `Some`, the session is seeded with it: a fresh /
    /// resumed launch passes it as claude's initial argument, and an
    /// already-open panel has it typed into the running conversation. Opening
    /// the panel never quits the shell.
    pub(crate) fn open_or_focus_brain(&mut self, prompt: Option<&str>) -> bool {
        // Already open with a live agent: reuse the existing session — focus
        // it and, if a prompt was supplied, type it into the running
        // conversation. We never spawn a second session while one is up.
        if self.brain.as_ref().is_some_and(PtyPane::is_alive) {
            self.focus = Panel::Brain;
            self.alert = None;
            if let Some(p) = prompt {
                if let Some(pty) = self.brain.as_ref() {
                    send_prompt_to_pty(pty, p);
                }
                self.mark_brain_turn_started();
                // The frontend-specific submit key is deferred a couple of
                // event-loop ticks (see `advance_submit_countdown`) so the
                // agent doesn't coalesce it into the pasted text.
                self.pending_brain_submit = BRAIN_SUBMIT_DELAY_TICKS;
            }
            return true;
        }
        // A panel whose agent died (between the loop's auto-close tick and
        // this call) is torn down first so we don't type into a dead PTY;
        // the resume path below picks the same session back up.
        if self.brain.is_some() {
            self.close_brain();
        }

        let pid = i32::try_from(std::process::id()).unwrap_or(0);
        let requested_actor = self.requested_receiver_actor.take();
        let receiver_request = requested_actor.is_some();
        let actor = requested_actor
            .unwrap_or_else(|| crate::actor::ActorContext::follow_up(&self.interactive_actor));
        let scope = crate::state::SessionScope::new(
            self.agent_kind,
            self.command_context.workspace.id(),
            actor.clone(),
        );
        let resume_override = self.receiver_resume_session.take();
        let mut resume = None;
        let mut skipped_missing = false;
        {
            let candidates =
                resume_override.map_or_else(|| self.db.sessions_by_recency(&scope), |id| vec![id]);
            for id in candidates {
                if self.agent_kind == AgentKind::Claude
                    && !session_transcript_exists(&self.brain_root, &id)
                {
                    skipped_missing = true;
                    continue;
                }
                if self.db.claim(&id, &self.instance, pid).unwrap_or(false) {
                    resume = Some(id);
                    break;
                }
            }
        }

        let new_id = uuid::Uuid::new_v4().to_string();
        let plan = Plan::decide(resume, new_id.clone());
        let session_id = match &plan {
            Plan::Resume(id) | Plan::Fresh(id) => id.clone(),
        };
        let response_id = if self.agent_kind == AgentKind::Claude {
            session_id
        } else {
            uuid::Uuid::new_v4().to_string()
        };
        if receiver_request {
            self.receiver_session_id = Some(response_id.clone());
            let response_path =
                self.command_context
                    .workspace
                    .paths()
                    .responses_dir()
                    .join(format!(
                        "{}.json",
                        self.receiver_session_id.as_deref().unwrap_or_default()
                    ));
            let _ = std::fs::remove_file(response_path);
        }
        if !receiver_request {
            self.interactive_session_id = Some(response_id.clone());
        }
        self.alert = if matches!(plan, Plan::Fresh(_)) {
            if self.agent_kind == AgentKind::Claude {
                let _ = self
                    .db
                    .register_scoped_fresh(&new_id, &self.instance, pid, &scope);
            }
            skipped_missing.then(|| {
                "⚠ couldn't find a session to resume — starting a new brain chat".to_owned()
            })
        } else {
            None
        };

        let llm_cmd = match self.agent_kind {
            AgentKind::Claude => crate::env::claude_command(&self.command_context),
            AgentKind::Codex => crate::env::codex_command(&self.command_context),
        };
        let command =
            session::build_llm_command(&self.brain_root, self.agent_kind, &llm_cmd, &plan, prompt);
        let env = session::env_for(
            &self.command_context.workspace,
            &actor,
            self.agent_kind,
            &self.instance,
            pid,
            &self.db_path,
            &response_id,
        );
        // Placeholder size; the first draw resizes the PTY to the real panel.
        match PtyPane::spawn_shell_command_with_env(&command, &env, &self.brain_root, 24, 80) {
            Ok(panel) => {
                self.brain = Some(panel);
                self.session_actor = Some(actor);
                self.brain_turn_active = false;
                if prompt.is_some_and(|value| !value.trim().is_empty()) {
                    self.mark_brain_turn_started();
                }
                self.focus = Panel::Brain;
                crate::logging::log(format!(
                    "brain panel started agent={} turn_active={}",
                    self.agent_kind.label(),
                    self.brain_turn_active
                ));
                true
            }
            Err(error) => {
                crate::logging::log(format!(
                    "brain panel start failed agent={} error={error:#}",
                    self.agent_kind.label()
                ));
                self.brain = None;
                self.brain_turn_active = false;
                self.receiver_session_id = None;
                self.session_actor = None;
                let _ = self.db.release(&self.instance);
                self.flash = Some(FlashKind::Error(format!(
                    "{} could not start: {error}",
                    self.agent_kind.label()
                )));
                false
            }
        }
    }

    pub(crate) fn mark_brain_turn_started(&mut self) {
        if self.receiver_lease.is_none()
            && let Some(session_id) = self.interactive_session_id.as_deref()
        {
            let path = self
                .command_context
                .workspace
                .paths()
                .responses_dir()
                .join(format!("{session_id}.json"));
            let _ = std::fs::remove_file(path);
        }
        if !self.brain_turn_active {
            crate::logging::log("brain turn started");
        }
        self.brain_turn_active = true;
    }

    /// Close the brain panel: drop the PTY (its Drop impl kills the agent
    /// child, ending the session process), release the session lock so a
    /// later open (or another shell) can resume it via recency, hand the
    /// screen back to full-width tasks, and reload so a brain action whose
    /// effect landed right before the close shows up immediately.
    pub(crate) fn close_brain(&mut self) {
        let receiver_panel = self.receiver_session_id.is_some();
        let completed_remote = self.brain.as_ref().is_some_and(|panel| !panel.is_alive())
            && receiver_panel
            && self.receiver_started.is_some();
        if completed_remote {
            if let (Some(panel), Some(channel), Some(sender)) = (
                self.brain.as_ref(),
                self.receiver_lease.map(|lease| lease.channel),
                self.receiver_sender.clone(),
            ) {
                let final_text = panel.contents();
                match channel {
                    crate::server::receiver::Channel::Sms => {
                        let reply = crate::server::reply::sms(&final_text);
                        crate::server::delivery::send_sms_background(
                            self.command_context.clone(),
                            "fallback final SMS response",
                            sender,
                            reply.text,
                        );
                    }
                    crate::server::receiver::Channel::Email => {
                        let recipients =
                            self.session_actor.as_ref().map_or_else(Vec::new, |actor| {
                                self.receiver_email_recipients(&self.receiver_recipients, actor)
                            });
                        if !recipients.is_empty() {
                            let reply = crate::server::reply::email(&final_text);
                            let html = crate::server::reply::email_html(&reply.text);
                            crate::server::delivery::send_email_background(
                                self.command_context.clone(),
                                "fallback final email response",
                                recipients,
                                "Brain response".to_owned(),
                                reply.text,
                                html,
                            );
                        }
                    }
                }
            }
        }
        self.brain = None;
        self.session_actor = None;
        self.brain_turn_active = false;
        self.pending_brain_submit = 0;
        self.alert = None;
        self.focus = Panel::Tasks;
        let _ = self.db.release(&self.instance);
        if receiver_panel {
            self.clear_receiver_panel_state();
        }
        self.reload_after_brain();
    }

    /// Re-read the CSVs after a brain interaction; route any error to
    /// the flash line so a transient load failure doesn't block the
    /// focus switch the user actually asked for.
    pub(crate) fn reload_after_brain(&mut self) {
        if let Err(e) = self.reload_tasks() {
            self.flash = Some(FlashKind::Error(format!("⚠ reload failed: {e}")));
        }
    }

    /// User-triggered refresh (the `r` hotkey). Re-reads the CSVs and
    /// flashes a confirmation so the user sees that the repaint
    /// actually happened, even when nothing visible changed.
    pub(crate) fn refresh(&mut self) {
        // A tasks session can span days. Advance `today` first if this refresh
        // crossed into a new logical day (6 AM rollover by default) so the
        // rebuilt view uses the new date, then re-open the daily-triage nudge
        // against the freshly-reloaded habits.
        let rolled = self.advance_triage_day(chrono::Local::now().naive_local());
        let reload = self.reload_tasks();
        if rolled {
            self.check_daily_triage();
        }
        self.flash = Some(match reload {
            Ok(()) => FlashKind::Info("✓ refreshed".to_string()),
            Err(e) => FlashKind::Error(format!("⚠ reload failed: {e}")),
        });
    }

    /// Advance the deferred-submit countdown one event-loop tick. When it
    /// reaches zero, send the frontend-specific submit key to the brain PTY for
    /// the prompt seeded a few ticks earlier. No-op when nothing is pending or
    /// the panel has since closed. Called once per event-loop iteration.
    pub(crate) fn tick_brain_submit(&mut self) {
        let (next, fire) = advance_submit_countdown(self.pending_brain_submit);
        self.pending_brain_submit = next;
        if fire {
            if let Some(pty) = self.brain.as_ref() {
                if pty.is_alive() {
                    pty.scroll_to_bottom();
                    pty.send(submit_key_for_agent(self.agent_kind));
                }
            }
        }
    }

    /// Send a prefilled prompt to the brain panel. Convenience wrapper that
    /// opens / focuses the panel (resuming the session) and seeds it with the
    /// prompt — used by palette actions like Defer, Start, Remove, and the
    /// agenda / triage flows.
    pub(crate) fn send_brain_prompt(&mut self, prompt: &str) {
        let trimmed = prompt.trim();
        if trimmed.is_empty() {
            return;
        }
        if self.receiver_panel_is_warm() {
            crate::logging::log(
                "local brain prompt leaving warm receiver session for interactive session",
            );
            self.close_receiver_panel(true);
        }
        self.open_or_focus_brain(Some(trimmed));
    }

    fn close_receiver_panel(&mut self, restore_interactive: bool) {
        self.brain = None;
        self.session_actor = None;
        self.brain_turn_active = false;
        self.pending_brain_submit = 0;
        self.alert = None;
        self.focus = Panel::Tasks;
        let _ = self.db.release(&self.instance);
        self.clear_receiver_panel_state();
        self.reload_after_brain();
        if restore_interactive {
            self.receiver_resume_session = (self.agent_kind == AgentKind::Claude)
                .then(|| self.interactive_session_id.take())
                .flatten();
            self.open_or_focus_brain(None);
        }
    }

    pub(crate) fn leave_warm_receiver_for_interactive_input(&mut self) {
        if self.receiver_panel_is_warm() {
            crate::logging::log(
                "keyboard input leaving warm receiver session for interactive session",
            );
            self.close_receiver_panel(true);
        }
    }

    fn receiver_panel_is_warm(&self) -> bool {
        self.receiver_session_id.is_some() && self.receiver_started.is_none()
    }

    fn clear_receiver_panel_state(&mut self) {
        self.receiver_sender = None;
        self.receiver_recipients.clear();
        self.receiver_session_id = None;
        self.receiver_lease = None;
        self.receiver_started = None;
        self.receiver_delay_sent = false;
        self.requested_receiver_actor = None;
    }
}

/// True if `claude --resume <id>` would actually find this session: a
/// transcript `<id>.jsonl` exists on disk. We check the brain project dir
/// first (where the tasks panel's sessions live, since it always runs claude
/// in `<brain_root>`), then fall back to scanning every project dir in case
/// claude's dir-mangling differs across versions.
pub(crate) fn session_transcript_exists(brain_root: &Path, id: &str) -> bool {
    let Some(home) = std::env::var_os("HOME") else {
        return false;
    };
    let base = PathBuf::from(home).join(".claude").join("projects");
    let file = format!("{id}.jsonl");
    if base
        .join(session::project_dir_name(brain_root))
        .join(&file)
        .is_file()
    {
        return true;
    }
    let Ok(entries) = std::fs::read_dir(&base) else {
        return false;
    };
    entries.flatten().any(|e| e.path().join(&file).is_file())
}

/// Type a prefilled prompt into an already-running agent PTY. Internal
/// newlines are sent as `Alt+Enter` (`ESC` + `CR`) — agent frontends treat
/// that as "insert newline", not "submit" — so a multi-line prompt arrives
/// intact. The submitting key is deliberately NOT appended here: frontends can
/// coalesce a burst of bytes ending in a submit key into a single paste, leaving
/// the message sitting unsent in the input. The caller defers the submit key a
/// couple of event-loop ticks (`App::pending_brain_submit` / `tick_brain_submit`)
/// so it arrives as a distinct keystroke and actually submits or queues.
pub(crate) fn send_prompt_to_pty(pty: &PtyPane, prompt: &str) {
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        return;
    }
    pty.scroll_to_bottom();
    let mut bytes: Vec<u8> = Vec::with_capacity(trimmed.len());
    for ch in trimmed.chars() {
        if ch == '\n' {
            bytes.extend_from_slice(&[0x1B, b'\r']);
        } else {
            let mut buf = [0u8; 4];
            bytes.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
        }
    }
    pty.send(bytes);
}

/// Keystroke that submits or queues an injected prompt for the active frontend.
#[must_use]
pub(crate) fn submit_key_for_agent(agent_kind: AgentKind) -> Vec<u8> {
    match agent_kind {
        AgentKind::Claude => vec![b'\r'],
        AgentKind::Codex => vec![b'\t'],
    }
}

/// Event-loop ticks to wait after seeding a prompt before sending the
/// frontend-specific submit key. Each tick is ~one poll interval (~50ms), so
/// two ticks puts a comfortable gap between the pasted text and the submit key.
const BRAIN_SUBMIT_DELAY_TICKS: u8 = 2;

/// Advance the deferred-submit countdown by one tick. Returns the new count
/// and whether the submitting key should be sent now — true only on the tick
/// the count reaches zero, so the key fires exactly once.
pub(crate) const fn advance_submit_countdown(pending: u8) -> (u8, bool) {
    match pending {
        0 => (0, false),
        1 => (0, true),
        n => (n - 1, false),
    }
}
