use std::sync::Arc;

use crate::agent::{HookMetadata, LaunchRequest, SessionStore};
use crate::session::Plan;
use crate::tui::*;

use super::brain_transport;

impl App<'_> {
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
            self.interactive_agent_session_id = None;
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
        let transport = brain_transport(self);
        let mut controller = self.controller_for_transport(actor.clone(), transport);
        if let Err(error) = controller.ensure_available() {
            crate::logging::log(format!("brain panel frontend unavailable: {error}"));
            self.flash = Some(FlashKind::Error(error.to_string()));
            return false;
        }
        let scope = crate::state::SessionScope::new(
            self.agent_kind,
            self.command_context.workspace.id(),
            actor.clone(),
        );
        let resume_override = self.receiver_resume_session.clone();
        // A `/new` sender asked to leave the previous conversation, so nothing
        // is offered for resumption; the fresh session registered below becomes
        // the most recent one and is what later messages resume instead.
        let force_fresh = std::mem::take(&mut self.receiver_force_fresh);
        let mut resume = None::<(String, String)>;
        let mut skipped_missing = false;
        {
            let candidates = if force_fresh {
                Vec::new()
            } else {
                resume_override.map_or_else(
                    || SessionStore::sessions_by_recency(&self.db, &scope),
                    |id| vec![id],
                )
            };
            for id in candidates {
                let Ok(candidate) = crate::agent::AgentSession::new(&id) else {
                    continue;
                };
                if !controller
                    .resume_candidate_exists(&candidate)
                    .unwrap_or(false)
                {
                    skipped_missing = true;
                    continue;
                }
                let response_id = match controller.response_id(&candidate) {
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
            None => match controller.response_id(&agent_session) {
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
            self.interactive_agent_session_id = Some(session_id);
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
                    self.interactive_agent_session_id = None;
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
}

pub(in crate::tui::app_brain) fn register_fresh_before_launch(
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
