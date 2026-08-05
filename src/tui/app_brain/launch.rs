//! Main-panel controller construction, session authorization, and semantic turns.

use crate::tui::*;

use std::sync::Arc;

use crossterm::event::KeyCode;

use crate::agent::{AgentController, HookMetadata, LaunchRequest, SessionStore};
use crate::pty_pane::PtyPane;
use crate::session::Plan;

#[cfg(not(test))]
fn brain_transport(_app: &mut App<'_>) -> Box<dyn crate::agent::AgentTransport> {
    Box::new(PtyPane::new(24, 80))
}

#[cfg(test)]
fn brain_transport(app: &mut App<'_>) -> Box<dyn crate::agent::AgentTransport> {
    app.brain_transport_override
        .take()
        .unwrap_or_else(|| Box::new(PtyPane::new(24, 80)))
}

impl App<'_> {
    pub(in crate::tui) fn launch_capability_plan(
        &self,
    ) -> anyhow::Result<Option<crate::access::CapabilityPlan>> {
        if self.config.access_mode == crate::access::AccessMode::Unrestricted {
            return Ok(None);
        }
        let mut config = crate::config::Config::try_load(&self.command_context.workspace)?;
        config.access_mode = self.config.access_mode;
        crate::access::capability_plan_for(&config, &self.command_context)
            .map(Some)
            .map_err(anyhow::Error::from)
    }

    pub(in crate::tui) fn controller_for_transport(
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

    /// Whether the brain panel is on screen (a live agent PTY).
    pub(crate) fn brain_panel_open(&self) -> bool {
        self.brain.is_some()
    }

    /// Handle the Ctrl-N shortcut before normal key forwarding. Returning
    /// `true` tells the event loop that the chord was consumed.
    pub(crate) fn handle_new_session_shortcut(&mut self, code: KeyCode, ctrl: bool) -> bool {
        if ctrl && matches!(code, KeyCode::Char('n' | 'N')) && self.any_brain_panel_visible() {
            let active_tab = self.effective_brain_tab();
            self.focus_brain();
            let started = self
                .active_brain_controller_mut()
                .is_some_and(|controller| controller.start_new_session().is_ok());
            if started && active_tab == BrainTab::Main {
                self.mark_brain_turn_started();
            }
            return true;
        }
        false
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
        // Already open with a live agent: reuse the existing session, focus
        // it and, if a prompt was supplied, type it into the running
        // conversation. We never spawn a second session while one is up.
        if self
            .brain
            .as_ref()
            .is_some_and(|controller| controller.is_alive().unwrap_or(false))
        {
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

        let receiver_request = self.requested_receiver_actor.is_some();
        if receiver_request {
            self.receiver_session_id = None;
        } else {
            self.interactive_session_id = None;
        }
        let frontend = crate::agent::configured_frontend(&self.command_context, self.agent_kind);
        if let Err(error) = frontend.ensure_available() {
            crate::logging::log(format!("brain panel frontend unavailable: {error}"));
            self.flash = Some(FlashKind::Error(error.to_string()));
            return false;
        }
        let capability_plan = match self.launch_capability_plan() {
            Ok(plan) => plan,
            Err(error) => {
                crate::logging::log(format!("brain panel capability resolution failed: {error}"));
                self.session_actor = None;
                self.flash = Some(FlashKind::Error(format!(
                    "agent capabilities are invalid: {error}"
                )));
                return false;
            }
        };

        let pid = i32::try_from(std::process::id()).unwrap_or(0);
        let requested_actor = self.requested_receiver_actor.clone();
        let actor = requested_actor
            .unwrap_or_else(|| crate::actor::ActorContext::follow_up(&self.interactive_actor));
        let scope = crate::state::SessionScope::new(
            self.agent_kind,
            self.command_context.workspace.id(),
            actor.clone(),
        );
        let resume_override = self.receiver_resume_session.clone();
        let mut resume = None::<(String, String)>;
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
                if !frontend
                    .resume_candidate_exists(&candidate)
                    .unwrap_or(false)
                {
                    skipped_missing = true;
                    continue;
                }
                let response_id = match frontend.response_id(&candidate) {
                    Ok(response_id) => response_id,
                    Err(error) => {
                        crate::logging::log(format!(
                            "brain panel response identity failed: {error}"
                        ));
                        self.session_actor = None;
                        self.flash = Some(FlashKind::Error(error.to_string()));
                        return false;
                    }
                };
                if SessionStore::claim(&self.db, &candidate, &self.instance, pid, &scope)
                    .unwrap_or(false)
                {
                    resume = Some((id, response_id));
                    break;
                }
            }
        }

        let resume_id = resume.as_ref().map(|(id, _)| id.clone());
        let new_id = uuid::Uuid::new_v4().to_string();
        let plan = Plan::decide(resume_id, new_id);
        let session_id = match &plan {
            Plan::Resume(id) | Plan::Fresh(id) => id.clone(),
        };
        let agent_session = crate::agent::AgentSession::new(&session_id)
            .expect("selected session IDs are non-blank");
        let response_id = match resume {
            Some((_, response_id)) => response_id,
            None => match frontend.response_id(&agent_session) {
                Ok(response_id) => response_id,
                Err(error) => {
                    crate::logging::log(format!("brain panel response identity failed: {error}"));
                    self.session_actor = None;
                    self.flash = Some(FlashKind::Error(error.to_string()));
                    return false;
                }
            },
        };
        self.requested_receiver_actor = None;
        self.receiver_resume_session = None;
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
        let fresh_session = matches!(plan, Plan::Fresh(_));
        self.alert = if fresh_session {
            skipped_missing.then(|| {
                "⚠ couldn't find a session to resume; starting a new brain chat".to_owned()
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
        let mut request = LaunchRequest::from_trusted_context(
            Arc::clone(&self.command_context.workspace),
            actor.clone(),
            session_plan,
            prompt.map(str::to_owned),
            self.config.access_mode,
        );
        if let Some(plan) = capability_plan {
            request = request.with_capability_plan(plan);
        }
        request = request.with_hook_metadata(hooks);
        let transport = brain_transport(self);
        let mut controller = AgentController::new(
            Arc::clone(&self.command_context.workspace),
            actor.clone(),
            frontend,
            transport,
        );
        // Placeholder size; the first draw resizes the PTY to the real panel.
        let launch_result = if fresh_session {
            register_fresh_before_launch(
                &self.db,
                request.session_plan().session(),
                &self.instance,
                pid,
                &scope,
                || controller.launch(&request),
            )
        } else {
            controller.launch(&request).map_err(anyhow::Error::from)
        };
        match launch_result {
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
                if receiver_request {
                    self.receiver_session_id = None;
                } else {
                    self.interactive_session_id = None;
                }
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
        if let Some(controller) = self.brain.as_ref() {
            let scope = crate::agent::SessionScope::new(
                controller.kind(),
                self.command_context.workspace.id(),
                controller.actor().clone(),
            );
            if let Err(error) = SessionStore::mark_active(&self.db, &self.instance, &scope) {
                crate::logging::log(format!("marking agent session active failed: {error:#}"));
            }
        }
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
    /// Send a prefilled prompt to the brain panel. Convenience wrapper that
    /// opens / focuses the panel (resuming the session) and seeds it with the
    /// prompt. Used by palette actions like Defer, Start, Remove, and the
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
}

pub(super) fn register_fresh_before_launch(
    store: &impl SessionStore,
    session: &crate::agent::AgentSession,
    instance: &str,
    pid: i32,
    scope: &crate::agent::SessionScope,
    launch: impl FnOnce() -> Result<(), crate::agent::AgentError>,
) -> anyhow::Result<()> {
    SessionStore::register(store, session, instance, pid, scope)
        .map_err(|error| anyhow::anyhow!("registering fresh agent session: {error:#}"))?;
    launch().map_err(anyhow::Error::from)
}
