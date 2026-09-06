use std::sync::Arc;

use anyhow::{Context as _, Result};

use crate::agent::{
    AgentController, AgentSession, HookMetadata, LaunchRequest, SessionPlan, SessionScope,
    SessionStore,
};
use crate::manual_session::{ManualSessionId, ManualSessionName, ManualSessionRecord};
use crate::tui::App;
use crate::tui::model::{BrainTab, SessionTabId};

pub(in crate::tui) enum ManualLaunchTarget {
    Main,
    Additional {
        record: ManualSessionRecord,
        tab_id: Option<SessionTabId>,
    },
}

struct PreparedManualLaunch {
    target: ManualLaunchTarget,
    request: LaunchRequest,
    controller: AgentController,
    response_id: String,
    resumed_session_id: Option<String>,
}

fn transport(app: &mut App, target: &ManualLaunchTarget) -> Box<dyn crate::agent::AgentTransport> {
    #[cfg(test)]
    {
        let injected = match target {
            ManualLaunchTarget::Main => app.brain.take_brain_transport(),
            ManualLaunchTarget::Additional { .. } => app.brain.take_manual_transport(),
        };
        if let Some(injected) = injected {
            return injected;
        }
    }
    #[cfg(not(test))]
    let _ = (app, target);
    Box::new(crate::pty_pane::PtyPane::new(24, 80))
}

impl App {
    pub(crate) fn start_manual_session(&mut self, name: ManualSessionName) {
        let result = self.new_manual_session_record(name).and_then(|record| {
            self.launch_manual_session(
                ManualLaunchTarget::Additional {
                    record,
                    tab_id: None,
                },
                None,
            )
        });
        match result {
            Ok(tab) => {
                self.select_brain_tab(tab);
            }
            Err(error) => self.report_manual_launch_error(&error),
        }
    }

    pub(in crate::tui) fn manual_session_scope(&self) -> SessionScope {
        SessionScope::new(
            self.context.agent_kind(),
            self.context.workspace().id(),
            crate::actor::ActorContext::follow_up(self.brain.interactive_actor()),
        )
    }

    fn new_manual_session_record(&self, name: ManualSessionName) -> Result<ManualSessionRecord> {
        let position = self
            .services
            .manual_sessions(&self.manual_session_scope())?
            .iter()
            .map(|record| record.position)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .context("manual session position exhausted")?;
        Ok(ManualSessionRecord::additional(
            ManualSessionId::new(),
            AgentSession::new(uuid::Uuid::new_v4().to_string())?,
            name,
            position,
        ))
    }

    pub(in crate::tui) fn report_manual_launch_error(&mut self, error: &anyhow::Error) {
        crate::logging::log(format!("manual session launch failed: {error:#}"));
        self.status.set_error(format!(
            "{} could not start: {error:#}",
            self.context.agent_kind().label()
        ));
    }

    pub(in crate::tui) fn launch_manual_session(
        &mut self,
        target: ManualLaunchTarget,
        prompt: Option<&str>,
    ) -> Result<BrainTab> {
        let prepared = self.prepare_manual_launch(target, prompt)?;
        self.spawn_manual_launch(prepared)
    }

    fn prepare_manual_launch(
        &mut self,
        target: ManualLaunchTarget,
        prompt: Option<&str>,
    ) -> Result<PreparedManualLaunch> {
        let capability_plan = self
            .launch_capability_plan()
            .context("agent capabilities are invalid")?;
        let actor = crate::actor::ActorContext::follow_up(self.brain.interactive_actor());
        let transport = transport(self, &target);
        let controller = self.controller_for_transport(actor.clone(), transport);
        controller.ensure_available()?;
        let scope = self.manual_session_scope();
        let records = self.services.manual_sessions(&scope)?;
        let id = match &target {
            ManualLaunchTarget::Main => self.brain.main_manual_session_id().clone(),
            ManualLaunchTarget::Additional { record, .. } => record.id.clone(),
        };
        let saved = records.iter().find(|record| record.id == id);
        let mut skipped_missing = false;
        let mut resumed_session_id = None;
        let mut response_id = None;
        if let Some(record) = saved {
            if !self.brain.resume_was_refused(record.agent_session.as_str())
                && controller.resume_candidate_exists(&record.agent_session)?
            {
                response_id = Some(controller.response_id(&record.agent_session)?);
                resumed_session_id = Some(record.agent_session.as_str().to_owned());
            } else {
                skipped_missing = true;
            }
        } else if matches!(target, ManualLaunchTarget::Main) {
            for candidate in SessionStore::sessions_by_recency(&self.services, &scope) {
                if records
                    .iter()
                    .any(|record| record.agent_session.as_str() == candidate)
                {
                    continue;
                }
                let Ok(session) = AgentSession::new(&candidate) else {
                    continue;
                };
                if self.brain.resume_was_refused(&candidate)
                    || !controller.resume_candidate_exists(&session)?
                {
                    skipped_missing = true;
                    continue;
                }
                response_id = Some(controller.response_id(&session)?);
                resumed_session_id = Some(candidate);
                break;
            }
        }
        let fresh = match &target {
            ManualLaunchTarget::Additional { record, .. } if saved.is_none() => {
                record.agent_session.clone()
            }
            _ => AgentSession::new(uuid::Uuid::new_v4().to_string())?,
        };
        let session_plan = SessionPlan::decide(
            resumed_session_id
                .as_deref()
                .map(AgentSession::new)
                .transpose()?,
            fresh,
        );
        let response_id =
            response_id.map_or_else(|| controller.response_id(session_plan.session()), Ok)?;
        let pid = std::process::id();
        let hooks = HookMetadata::new(vec![
            ("BRAIN_INSTANCE_ID".to_owned(), id.as_str().to_owned()),
            ("BRAIN_PID".to_owned(), pid.to_string()),
            (
                "BRAIN_STATE_DB".to_owned(),
                self.context.state_db_path().display().to_string(),
            ),
            ("BRAIN_RESPONSE_ID".to_owned(), response_id.clone()),
            (
                "BRAIN_RESPONSE_DIR".to_owned(),
                self.context
                    .workspace()
                    .paths()
                    .responses_dir()
                    .display()
                    .to_string(),
            ),
        ]);
        let mut request = LaunchRequest::from_trusted_context(
            Arc::clone(&self.context.command().workspace),
            actor,
            session_plan,
            prompt.map(str::to_owned),
            self.context.access_mode(),
        )
        .with_hook_metadata(hooks);
        if let Some(plan) = capability_plan {
            request = request.with_capability_plan(plan);
        }
        if matches!(target, ManualLaunchTarget::Main) {
            self.status
                .set_alert((skipped_missing && resumed_session_id.is_none()).then(|| {
                    "⚠ couldn't find a session to resume; starting a new brain chat".to_owned()
                }));
        }
        Ok(PreparedManualLaunch {
            target,
            request,
            controller,
            response_id,
            resumed_session_id,
        })
    }

    fn spawn_manual_launch(&mut self, prepared: PreparedManualLaunch) -> Result<BrainTab> {
        let PreparedManualLaunch {
            target,
            request,
            mut controller,
            response_id,
            resumed_session_id,
        } = prepared;
        let scope = self.manual_session_scope();
        let pid = i32::try_from(std::process::id()).context("process id is out of range")?;
        let record = match &target {
            ManualLaunchTarget::Main => ManualSessionRecord::main(
                self.brain.main_manual_session_id().clone(),
                request.session_plan().session().clone(),
            ),
            ManualLaunchTarget::Additional { record, .. } => ManualSessionRecord {
                agent_session: request.session_plan().session().clone(),
                ..record.clone()
            },
        };
        let saved = self
            .services
            .manual_sessions(&scope)?
            .iter()
            .any(|saved| saved.id == record.id);
        if resumed_session_id.is_some() {
            anyhow::ensure!(
                SessionStore::claim(
                    &self.services,
                    &record.agent_session,
                    record.id.as_str(),
                    pid,
                    &scope
                )?,
                "manual session is already in use"
            );
            if !saved && let Err(error) = self.services.attach_manual_session(&record, &scope) {
                let _ = SessionStore::release(&self.services, record.id.as_str());
                return Err(error);
            }
        } else if saved {
            self.services
                .replace_manual_session(&record.id, &record.agent_session, pid, &scope)?;
        } else {
            self.services
                .register_fresh_manual_session(&record, pid, &scope)?;
        }
        if let Err(error) = controller.launch(&request) {
            let _ = controller.shutdown();
            let cleanup = if !saved && matches!(target, ManualLaunchTarget::Additional { .. }) {
                self.services
                    .rollback_fresh_manual_session(&record, pid, &scope)
            } else {
                self.services.release_manual_session(&record.id, &scope)
            };
            if let Err(cleanup) = cleanup {
                crate::logging::log(format!("manual session launch cleanup failed: {cleanup:#}"));
            }
            return Err(error.into());
        }
        match target {
            ManualLaunchTarget::Main => {
                self.brain.record_interactive_session_started(
                    response_id,
                    record.agent_session.as_str().to_owned(),
                );
                self.brain.install_main(controller);
                self.brain
                    .arm_main_startup(resumed_session_id, self.services.monotonic_now());
                if request
                    .initial_prompt()
                    .is_some_and(|prompt| !prompt.trim().is_empty())
                {
                    self.mark_brain_turn_started();
                }
                Ok(BrainTab::Main)
            }
            ManualLaunchTarget::Additional { tab_id, .. } => {
                let manual_id = record.id.clone();
                let attached = if let Some(id) = tab_id {
                    self.brain
                        .replace_manual_controller(id, &record, controller, resumed_session_id)
                        .map(|()| id)
                } else {
                    self.brain
                        .add_manual_session(record, controller, resumed_session_id)
                        .map_err(anyhow::Error::from)
                };
                match attached {
                    Ok(id) => {
                        self.brain
                            .arm_manual_startup(id, self.services.monotonic_now());
                        Ok(BrainTab::Session(id))
                    }
                    Err(error) => {
                        self.services.release_manual_session(&manual_id, &scope)?;
                        Err(error)
                    }
                }
            }
        }
    }
}
