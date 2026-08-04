//! `App` brain-panel lifecycle: open/resume, close, focus, and seeding the
//! session with a prefilled prompt.
//!
//! The brain panel is a persistent, resumable agent session (the same model as
//! the `brain` sibling shell): opening it resumes the
//! most-recently-active free session for this shell (lock + recency), or
//! starts a fresh one; closing it ends that agent process but leaves the
//! conversation resumable next time. There is no completion view and no
//! queue — the panel simply stays open until the user closes it (Ctrl+X /
//! "Close brain") or the agent exits.

use super::*;

use std::sync::Arc;

use crossterm::event::KeyCode;

use crate::agent::{AccessPolicy, AgentController, HookMetadata, LaunchRequest, SessionStore};
use crate::pty_pane::PtyPane;
use crate::session::Plan;

impl App<'_> {
    pub(super) fn controller_for_transport(
        &self,
        actor: crate::actor::ActorContext,
        transport: Box<dyn crate::agent::AgentTransport>,
    ) -> AgentController {
        AgentController::new(
            Arc::clone(&self.command_context.workspace),
            actor,
            crate::agent::configured_frontend(&self.command_context, self.agent_kind),
            transport,
        )
    }

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

    /// Handle the Ctrl-N shortcut before normal key forwarding. Returning
    /// `true` tells the event loop that the chord was consumed.
    pub(crate) fn handle_new_session_shortcut(&mut self, code: KeyCode, ctrl: bool) -> bool {
        if ctrl && matches!(code, KeyCode::Char('n' | 'N')) && self.brain_panel_open() {
            self.focus_brain();
            if let Some(controller) = self.brain.as_mut()
                && controller.start_new_session().is_ok()
            {
                self.mark_brain_turn_started();
            }
            return true;
        }
        false
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
    /// resumed launch passes it as the agent's initial argument, and an
    /// already-open panel has it typed into the running conversation. Opening
    /// the panel never quits the shell.
    pub(crate) fn open_or_focus_brain(&mut self, prompt: Option<&str>) -> bool {
        // Already open with a live agent: reuse the existing session — focus
        // it and, if a prompt was supplied, type it into the running
        // conversation. We never spawn a second session while one is up.
        if self.brain.as_ref().is_some_and(AgentController::is_alive) {
            self.focus = Panel::Brain;
            self.alert = None;
            if let Some(p) = prompt {
                if let Some(controller) = self.brain.as_mut()
                    && let Err(error) = controller.queue_after_active_turn(p)
                {
                    crate::logging::log(format!("brain prompt queue failed: {error}"));
                    return false;
                }
                self.mark_brain_turn_started();
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
        let frontend = crate::agent::configured_frontend(&self.command_context, self.agent_kind);
        let mut resume = None;
        let mut skipped_missing = false;
        {
            let candidates = resume_override.map_or_else(
                || SessionStore::sessions_by_recency(&self.db, &scope),
                |id| vec![id],
            );
            for id in candidates {
                let Ok(candidate) = crate::agent::AgentSession::new(&id) else {
                    continue;
                };
                if !frontend.resume_candidate_exists(&candidate) {
                    skipped_missing = true;
                    continue;
                }
                if SessionStore::claim(&self.db, &candidate, &self.instance, pid, &scope)
                    .unwrap_or(false)
                {
                    resume = Some(id);
                    break;
                }
            }
        }

        let new_id = uuid::Uuid::new_v4().to_string();
        let plan = Plan::decide(resume, new_id);
        let session_id = match &plan {
            Plan::Resume(id) | Plan::Fresh(id) => id.clone(),
        };
        let agent_session = crate::agent::AgentSession::new(&session_id)
            .expect("selected session IDs are non-blank");
        let response_id = frontend.response_id(&agent_session);
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
            let _ = SessionStore::register(&self.db, &agent_session, &self.instance, pid, &scope);
            skipped_missing.then(|| {
                "⚠ couldn't find a session to resume — starting a new brain chat".to_owned()
            })
        } else {
            None
        };

        let session_plan = match plan {
            Plan::Resume(_) => crate::agent::SessionPlan::resume(agent_session),
            Plan::Fresh(_) => crate::agent::SessionPlan::fresh(agent_session),
        };
        let hooks = HookMetadata::new(vec![
            ("BRAIN_INSTANCE_ID".to_owned(), self.instance.clone()),
            ("BRAIN_PID".to_owned(), pid.to_string()),
            (
                "BRAIN_STATE_DB".to_owned(),
                self.db_path.display().to_string(),
            ),
            ("BRAIN_RESPONSE_ID".to_owned(), response_id),
            (
                "BRAIN_RESPONSE_DIR".to_owned(),
                self.command_context
                    .workspace
                    .paths()
                    .responses_dir()
                    .display()
                    .to_string(),
            ),
        ]);
        let request = LaunchRequest::new(
            Arc::clone(&self.command_context.workspace),
            actor.clone(),
            session_plan,
            prompt.map(str::to_owned),
            AccessPolicy::default(),
        )
        .with_hook_metadata(hooks);
        let mut controller = AgentController::new(
            Arc::clone(&self.command_context.workspace),
            actor.clone(),
            frontend,
            Box::new(PtyPane::new(24, 80)),
        );
        // Placeholder size; the first draw resizes the PTY to the real panel.
        match controller.launch(&request) {
            Ok(()) => {
                self.brain = Some(controller);
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
                let _ = SessionStore::release(&self.db, &self.instance);
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
        self.close_brain_with(Self::deliver_completed_remote_turn);
    }

    fn close_brain_with(
        &mut self,
        deliver: impl FnOnce(&mut Self, crate::server::delivery::CompletionDelivery),
    ) {
        let receiver_panel = self.receiver_session_id.is_some();
        let completed_remote = self.brain.as_ref().is_some_and(|panel| !panel.is_alive())
            && receiver_panel
            && self.receiver_started.is_some();
        let completion = completed_remote
            .then_some(self.brain.as_ref())
            .flatten()
            .and_then(crate::server::delivery::CompletionDelivery::capture);
        if let Some(completion) = completion {
            deliver(self, completion);
        }
        if let Some(mut controller) = self.brain.take() {
            controller.shutdown();
        }
        self.session_actor = None;
        self.brain_turn_active = false;
        self.alert = None;
        self.focus = Panel::Tasks;
        let _ = SessionStore::release(&self.db, &self.instance);
        if receiver_panel {
            self.clear_receiver_panel_state();
        }
        self.reload_after_brain();
    }

    fn deliver_completed_remote_turn(
        &mut self,
        completion: crate::server::delivery::CompletionDelivery,
    ) {
        let (snapshot, actor, channel) = completion.into_parts();
        let Some(sender) = self.receiver_sender.clone() else {
            return;
        };
        match channel {
            crate::server::receiver::Channel::Sms => {
                let reply = crate::server::reply::sms(&snapshot);
                crate::server::delivery::send_sms_background(
                    self.command_context.clone(),
                    "fallback final SMS response",
                    sender,
                    reply.text,
                );
            }
            crate::server::receiver::Channel::Email => {
                let recipients = self.receiver_email_recipients(&self.receiver_recipients, &actor);
                if !recipients.is_empty() {
                    let reply = crate::server::reply::email(&snapshot);
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

    pub(crate) fn close_exited_brain_panel(&mut self) -> bool {
        if self
            .brain
            .as_ref()
            .is_some_and(|controller| !controller.is_alive())
        {
            self.close_brain();
            return true;
        }
        false
    }

    /// Advance frontend-neutral delayed controller input for live panels.
    pub(crate) fn tick_agent_controllers(&mut self) {
        for controller in [&mut self.brain, &mut self.triage_brain]
            .into_iter()
            .flatten()
        {
            if let Err(error) = controller.tick() {
                crate::logging::log(format!("agent input delivery failed: {error}"));
            }
        }
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
        let can_resume = self
            .brain
            .as_ref()
            .is_some_and(AgentController::can_resume_response_session);
        if let Some(mut controller) = self.brain.take() {
            controller.shutdown();
        }
        self.session_actor = None;
        self.brain_turn_active = false;
        self.alert = None;
        self.focus = Panel::Tasks;
        let _ = SessionStore::release(&self.db, &self.instance);
        self.clear_receiver_panel_state();
        self.reload_after_brain();
        if restore_interactive {
            self.receiver_resume_session = can_resume
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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    use chrono::NaiveDate;
    use clap::Parser;

    use super::*;
    use crate::agent::{
        AgentController, AgentError, AgentFrontend, AgentSession, AgentTransport,
        CompletionStrategy, HookMetadata, InputSequence, LaunchRequest, LaunchSpec,
    };
    use crate::config::Config;
    use crate::pty_pane::PtyPane;
    use crate::server::receiver::{Channel, InboundMessage};
    use crate::session;
    use crate::session::AgentKind;
    use crate::state::{Db, SessionScope};
    use crate::tasks::cli::Cli;
    use crate::tasks::selector::Selector;
    use crate::tasks::task::AssignmentContext;
    use crate::tasks::view::{View, build_view};
    use crate::tui::{Panel, PanelSide, ZshFunctionRunner};
    use crate::workspace::{
        CommandContext, RegistryStore, WorkspaceContext, WorkspaceId, WorkspaceName,
    };

    const WORKSPACE_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum ControllerEvent {
        SubmitNow,
        QueueAfterActiveTurn,
        QueueDelivered,
        Shutdown,
    }

    #[derive(Clone, Default)]
    struct ControllerRecording(Arc<Mutex<Vec<ControllerEvent>>>);

    impl ControllerRecording {
        fn record(&self, event: ControllerEvent) {
            self.0.lock().expect("controller recording").push(event);
        }

        fn events(&self) -> Vec<ControllerEvent> {
            self.0.lock().expect("controller recording").clone()
        }
    }

    struct RecordingFrontend {
        recording: ControllerRecording,
    }

    impl AgentFrontend for RecordingFrontend {
        fn kind(&self) -> AgentKind {
            AgentKind::Claude
        }

        fn launch_spec(&self, request: &LaunchRequest) -> Result<LaunchSpec, AgentError> {
            Ok(LaunchSpec::new(
                "recording-agent",
                request.workspace().root().to_path_buf(),
                Vec::new(),
                HookMetadata::none(),
            ))
        }

        fn submit_input(&self) -> InputSequence {
            self.recording.record(ControllerEvent::SubmitNow);
            InputSequence::bytes(b"\r")
        }

        fn queue_input(&self) -> InputSequence {
            self.recording.record(ControllerEvent::QueueAfterActiveTurn);
            InputSequence::bytes(b"\x1dqueue")
        }

        fn new_session_input(&self) -> InputSequence {
            InputSequence::bytes(b"/new\r")
        }

        fn completion_strategy(&self) -> CompletionStrategy {
            CompletionStrategy::Hook
        }

        fn transcript(&self, _session: &AgentSession) -> Option<PathBuf> {
            None
        }

        fn resume_candidate_exists(&self, _session: &AgentSession) -> bool {
            true
        }

        fn response_id(&self, session: &AgentSession) -> String {
            session.as_str().to_owned()
        }

        fn can_resume_response_session(&self) -> bool {
            true
        }
    }

    struct RecordingTransport {
        recording: ControllerRecording,
        alive: bool,
        snapshot: String,
    }

    impl AgentTransport for RecordingTransport {
        fn spawn(&mut self, _spec: &LaunchSpec) -> Result<(), AgentError> {
            self.alive = true;
            Ok(())
        }

        fn send(&mut self, input: InputSequence) -> Result<(), AgentError> {
            if input.into_bytes().ends_with(b"\x1dqueue") {
                self.recording.record(ControllerEvent::QueueDelivered);
            }
            Ok(())
        }

        fn snapshot(&self) -> String {
            self.snapshot.clone()
        }

        fn is_alive(&self) -> bool {
            self.alive
        }

        fn shutdown(&mut self) {
            self.recording.record(ControllerEvent::Shutdown);
            self.alive = false;
        }
    }

    fn recording_controller(
        app: &App<'_>,
        alive: bool,
        snapshot: &str,
    ) -> (AgentController, ControllerRecording) {
        recording_controller_for_actor(app, app.interactive_actor.clone(), alive, snapshot)
    }

    fn recording_controller_for_actor(
        app: &App<'_>,
        actor: crate::actor::ActorContext,
        alive: bool,
        snapshot: &str,
    ) -> (AgentController, ControllerRecording) {
        let recording = ControllerRecording::default();
        let controller = AgentController::new(
            Arc::clone(&app.command_context.workspace),
            actor,
            Box::new(RecordingFrontend {
                recording: recording.clone(),
            }),
            Box::new(RecordingTransport {
                recording: recording.clone(),
                alive,
                snapshot: snapshot.to_owned(),
            }),
        );
        (controller, recording)
    }

    fn test_app<'a>(temporary: &tempfile::TempDir, cli: &'a Cli, agent_kind: AgentKind) -> App<'a> {
        let root = temporary.path().join("family");
        std::fs::create_dir_all(root.join("tasks")).expect("create task directory");
        std::fs::create_dir_all(root.join(".config")).expect("create config directory");
        std::fs::write(
            root.join("tasks/tasks.csv"),
            "task_uuid,task_id,task_name,status,assigned_to,system_key\n",
        )
        .expect("write tasks");
        std::fs::write(
            root.join("tasks/habits.csv"),
            "task_uuid,task_id,task_name,status,assigned_to,system_key\n",
        )
        .expect("write habits");
        std::fs::write(
            root.join(".config/config.json"),
            "{\"claude_cmd\":\"sh -c 'sleep 30' #\"}\n",
        )
        .expect("write test agent command");
        let workspace = WorkspaceContext::new(
            temporary.path(),
            WorkspaceId::parse(WORKSPACE_ID).expect("valid workspace id"),
            WorkspaceName::parse("family").expect("valid workspace name"),
            &root,
            "pablo",
            temporary.path(),
        )
        .expect("workspace context");
        let context = CommandContext::for_test(
            Arc::new(workspace),
            RegistryStore::from_path(temporary.path().join("env.json")),
            "pablo",
        );
        let today = NaiveDate::from_ymd_opt(2026, 8, 4).expect("valid date");
        let view = build_view(cli, &Selector::All, Some(View::All), Vec::new(), today);
        let assignment = AssignmentContext::legacy(&context.actor);
        let db = Db::open(&context.workspace).expect("state db");
        App::new(
            context,
            &view,
            cli,
            today,
            root.join("tasks/tasks.csv"),
            Vec::new(),
            Vec::new(),
            assignment,
            None,
            Some(View::All),
            None,
            Box::new(ZshFunctionRunner::new("")),
            Box::new(ZshFunctionRunner::new("")),
            Config {
                enable_triage_habits: false,
                ..Config::default()
            },
            agent_kind,
            "shell-under-test".to_owned(),
            db,
            crate::picker::App::new(&[], ""),
            PanelSide::Right,
            true,
        )
    }

    fn sms_actor() -> crate::actor::ActorContext {
        let users = crate::users::Users {
            schema_version: crate::users::USERS_SCHEMA_VERSION,
            users: vec![crate::users::User {
                id: crate::users::UserId::parse("remote-member").unwrap(),
                name: "Remote member".to_owned(),
                phones: vec![crate::users::PhoneIdentity {
                    value: "+15551234567".to_owned(),
                    inbound_allowed: true,
                }],
                emails: Vec::new(),
                response_email: None,
            }],
        };
        crate::actor::resolve_actor(
            &crate::users::UserId::parse("remote-member").unwrap(),
            crate::actor::RequestIdentity::Sms {
                from: "+15551234567",
            },
            &users,
        )
        .unwrap()
    }

    fn live_panel(root: &Path) -> PtyPane {
        PtyPane::spawn_shell_command_with_env("cat", &[], root, 24, 80).expect("spawn panel")
    }

    fn panel_controller(app: &App<'_>, panel: PtyPane) -> AgentController {
        AgentController::new(
            Arc::clone(&app.command_context.workspace),
            app.interactive_actor.clone(),
            crate::agent::configured_frontend(&app.command_context, app.agent_kind),
            Box::new(panel),
        )
    }

    #[test]
    fn controller_drives_interactive_submit_queued_work_and_single_shutdown() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&temporary, &cli, AgentKind::Claude);
        let (controller, recording) = recording_controller(&app, true, "final snapshot");
        app.brain = Some(controller);
        app.focus = Panel::Brain;

        let enter =
            crossterm::event::KeyEvent::new(KeyCode::Enter, crossterm::event::KeyModifiers::NONE);
        handle_brain_key(&mut app, &enter, false);
        app.send_brain_prompt("queued inbound work");

        assert_eq!(
            recording.events(),
            vec![
                ControllerEvent::SubmitNow,
                ControllerEvent::QueueAfterActiveTurn,
            ]
        );

        app.tick_agent_controllers();
        app.tick_agent_controllers();
        app.close_brain();
        app.close_brain();

        assert_eq!(
            recording.events(),
            vec![
                ControllerEvent::SubmitNow,
                ControllerEvent::QueueAfterActiveTurn,
                ControllerEvent::QueueDelivered,
                ControllerEvent::Shutdown,
            ]
        );
    }

    #[test]
    fn agent_exit_closes_only_the_panel_and_returns_to_the_live_tui() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&temporary, &cli, AgentKind::Claude);
        let (controller, recording) = recording_controller(&app, false, "final snapshot");
        app.brain = Some(controller);
        app.focus = Panel::Brain;

        assert!(app.close_exited_brain_panel());

        assert!(app.brain.is_none());
        assert_eq!(app.focus, Panel::Tasks);
        assert_eq!(recording.events(), vec![ControllerEvent::Shutdown]);
    }

    #[test]
    fn close_delivers_transport_snapshot_with_the_initiating_actor_and_channel() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&temporary, &cli, AgentKind::Claude);
        let initiating_actor = sms_actor();
        let (controller, _) = recording_controller_for_actor(
            &app,
            initiating_actor.clone(),
            false,
            "remote transport snapshot",
        );
        app.brain = Some(controller);
        app.session_actor = Some(app.interactive_actor.clone());
        app.receiver_session_id = Some("receiver-session".to_owned());
        app.receiver_started = Some(std::time::Instant::now());
        app.receiver_sender = Some("+15551234567".to_owned());
        app.receiver_lease = Some(crate::tui::receiver_state::renew(
            Channel::Email,
            0,
            std::time::Instant::now(),
        ));
        let mut delivered = None;

        app.close_brain_with(|_, completion| delivered = Some(completion));

        let delivered = delivered.expect("completion delivered before teardown");
        let (snapshot, actor, channel) = delivered.into_parts();
        assert_eq!(snapshot, "remote transport snapshot");
        assert_eq!(actor, initiating_actor);
        assert_eq!(channel, Channel::Sms);
        assert!(app.brain.is_none());
        assert_eq!(app.focus, Panel::Tasks);
    }

    #[test]
    fn normal_and_triage_controllers_use_the_same_selected_adapter() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let cli = Cli::parse_from(["tasks"]);

        for kind in [AgentKind::Claude, AgentKind::Codex] {
            let app = test_app(&temporary, &cli, kind);
            let normal = app.controller_for_transport(
                app.interactive_actor.clone(),
                Box::new(RecordingTransport {
                    recording: ControllerRecording::default(),
                    alive: false,
                    snapshot: String::new(),
                }),
            );
            let triage = app.controller_for_transport(
                app.interactive_actor.clone(),
                Box::new(RecordingTransport {
                    recording: ControllerRecording::default(),
                    alive: false,
                    snapshot: String::new(),
                }),
            );

            assert_eq!(normal.kind(), kind);
            assert_eq!(triage.kind(), kind);
        }
    }

    fn capture_panel(root: &Path) -> PtyPane {
        PtyPane::spawn_shell_command_with_env(
            "stty raw -echo; printf READY; dd bs=1 count=5 2>/dev/null | od -An -t x1",
            &[],
            root,
            24,
            80,
        )
        .expect("spawn capture panel")
    }

    fn wait_for_panel_contents(panel: &AgentController, expected: &str) -> bool {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let normalized = panel
                .snapshot()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if normalized.contains(expected) {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    struct ClaudeTranscript {
        path: PathBuf,
        project_dir: PathBuf,
    }

    impl ClaudeTranscript {
        fn create(brain_root: &Path, session_id: &str) -> Self {
            let home = std::env::var_os("HOME").expect("test home directory");
            let project_dir = PathBuf::from(home)
                .join(".claude/projects")
                .join(session::project_dir_name(brain_root));
            std::fs::create_dir_all(&project_dir).expect("create transcript directory");
            let path = project_dir.join(format!("{session_id}.jsonl"));
            std::fs::write(&path, "{}\n").expect("write Claude transcript");
            Self { path, project_dir }
        }
    }

    impl Drop for ClaudeTranscript {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
            let _ = std::fs::remove_dir(&self.project_dir);
        }
    }

    #[test]
    fn app_session_selection_skips_missing_claude_transcripts_and_claims_valid_resume() {
        let cli = Cli::parse_from(["tasks"]);
        let resume_temporary = tempfile::tempdir().expect("resume temporary directory");
        let mut resume_app = test_app(&resume_temporary, &cli, AgentKind::Claude);
        let resume_scope = SessionScope::new(
            AgentKind::Claude,
            resume_app.command_context.workspace.id(),
            resume_app.interactive_actor.clone(),
        );
        let valid_id = "valid-resume";
        let missing_id = "missing-resume";
        for id in [valid_id, missing_id] {
            resume_app
                .db
                .register_scoped_fresh(id, "prior-shell", 42, &resume_scope)
                .expect("register candidate");
            resume_app
                .db
                .release("prior-shell")
                .expect("release candidate");
        }
        let _transcript =
            ClaudeTranscript::create(resume_app.command_context.workspace.root(), valid_id);

        assert!(resume_app.open_or_focus_brain(None));

        assert_eq!(resume_app.interactive_session_id.as_deref(), Some(valid_id));
        assert!(resume_app.alert.is_none());
        assert_eq!(
            resume_app.db.sessions_by_recency(&resume_scope),
            [missing_id]
        );

        let fresh_temporary = tempfile::tempdir().expect("fresh temporary directory");
        let mut fresh_app = test_app(&fresh_temporary, &cli, AgentKind::Claude);
        let fresh_scope = SessionScope::new(
            AgentKind::Claude,
            fresh_app.command_context.workspace.id(),
            fresh_app.interactive_actor.clone(),
        );
        fresh_app
            .db
            .register_scoped_fresh(missing_id, "prior-shell", 42, &fresh_scope)
            .expect("register missing candidate");
        fresh_app
            .db
            .release("prior-shell")
            .expect("release missing candidate");

        assert!(fresh_app.open_or_focus_brain(None));

        assert_ne!(
            fresh_app.interactive_session_id.as_deref(),
            Some(missing_id)
        );
        assert!(
            fresh_app
                .alert
                .as_deref()
                .is_some_and(|message| message.contains("couldn't find a session to resume"))
        );
        assert_eq!(fresh_app.db.sessions_by_recency(&fresh_scope), [missing_id]);
    }

    #[test]
    fn ctrl_n_routes_new_session_through_the_selected_controller_adapter() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let cli = Cli::parse_from(["tasks"]);

        for agent_kind in [AgentKind::Claude, AgentKind::Codex] {
            let mut app = test_app(&temporary, &cli, agent_kind);
            let capture = capture_panel(app.command_context.workspace.root());
            app.brain = Some(panel_controller(&app, capture));
            assert!(
                wait_for_panel_contents(app.brain.as_ref().expect("panel"), "READY"),
                "capture panel did not become ready"
            );

            assert!(!app.handle_new_session_shortcut(KeyCode::Char('n'), false));
            assert!(app.handle_new_session_shortcut(KeyCode::Char('n'), true));
            assert_eq!(app.focus, Panel::Brain);
            assert!(app.brain_turn_active);
            let expected_bytes = match agent_kind {
                AgentKind::Claude => "2f 6e 65 77 0d",
                AgentKind::Codex => "2f 6e 65 77 09",
            };
            let panel = app
                .brain
                .as_ref()
                .expect("panel remains open until capture exits");
            assert!(
                wait_for_panel_contents(panel, expected_bytes),
                "capture panel did not receive deferred /new bytes: {}",
                panel.snapshot()
            );
        }
    }

    #[test]
    fn receiver_queue_reuses_the_matching_warm_session_through_app_dispatch() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&temporary, &cli, AgentKind::Claude);
        let actor = app.interactive_actor.clone();
        let live = live_panel(app.command_context.workspace.root());
        app.brain = Some(panel_controller(&app, live));
        app.session_actor = Some(actor.clone());
        app.receiver_session_id = Some("receiver-session".to_owned());
        app.receiver_lease = Some(crate::tui::receiver_state::renew(
            Channel::Sms,
            0,
            std::time::Instant::now(),
        ));
        app.receiver_queue.push(InboundMessage {
            workspace_id: app.command_context.workspace.id(),
            actor: actor.clone(),
            channel: Channel::Sms,
            body: "continue this conversation".to_owned(),
            sender: "+15551234567".to_owned(),
            participants: vec!["+15551234567".to_owned()],
            provider_id: Some("provider-message-1".to_owned()),
            attachments: Vec::new(),
        });

        app.tick_receiver();

        assert!(app.receiver_queue.is_empty());
        assert_eq!(app.receiver_session_id.as_deref(), Some("receiver-session"));
        assert_eq!(app.session_actor.as_ref(), Some(&actor));
        assert!(app.receiver_started.is_some());
        assert!(app.brain_turn_active);
    }

    #[test]
    fn close_brain_releases_each_frontend_session_for_the_next_shell() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let cli = Cli::parse_from(["tasks"]);

        for agent_kind in [AgentKind::Claude, AgentKind::Codex] {
            let mut app = test_app(&temporary, &cli, agent_kind);
            let scope = SessionScope::new(
                agent_kind,
                app.command_context.workspace.id(),
                app.interactive_actor.clone(),
            );
            let session_id = format!("{agent_kind:?}-session");
            app.db
                .register_scoped_fresh(&session_id, &app.instance, 42, &scope)
                .expect("register locked session");
            let live = live_panel(app.command_context.workspace.root());
            app.brain = Some(panel_controller(&app, live));
            app.focus = Panel::Brain;

            app.close_brain();

            assert!(app.brain.is_none());
            assert_eq!(app.focus, Panel::Tasks);
            assert_eq!(app.db.sessions_by_recency(&scope), [session_id]);
        }
    }
}
