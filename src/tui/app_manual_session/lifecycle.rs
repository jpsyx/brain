use anyhow::Context as _;

use crate::agent::SessionStore;
use crate::tui::App;
use crate::tui::app_brain::launch::arrival::ExitedPanel;
use crate::tui::model::{BrainTab, SessionTabId};

use super::ManualLaunchTarget;

impl App {
    pub(crate) fn rename_manual_session(
        &mut self,
        tab_id: SessionTabId,
        name: &crate::manual_session::ManualSessionName,
    ) -> anyhow::Result<()> {
        let id = self
            .brain
            .manual_session_id(tab_id)
            .cloned()
            .context("manual session was not found")?;
        let record =
            self.services
                .rename_manual_session(&id, name, &self.manual_session_scope())?;
        self.brain.rename_manual_session(tab_id, &record)
    }

    pub(crate) fn close_manual_session(&mut self, id: SessionTabId) {
        let Some(manual_id) = self.brain.manual_session_id(id).cloned() else {
            return;
        };
        if let Some(controller) = self.brain.active_controller_mut(BrainTab::Session(id))
            && let Err(error) = controller.shutdown()
        {
            self.status
                .set_error(format!("session could not close: {error}"));
            return;
        }
        if let Err(error) = self
            .services
            .close_manual_session(&manual_id, &self.manual_session_scope())
        {
            self.status
                .set_error(format!("session could not close: {error}"));
            return;
        }
        let was_active = self.effective_brain_tab() == BrainTab::Session(id);
        let removed = self.brain.remove_manual_session(id);
        debug_assert!(removed.is_some_and(|removed| removed.id == manual_id));
        if was_active {
            self.select_brain_tab(BrainTab::Main);
        }
        self.reload_after_brain();
    }

    pub(crate) fn close_active_user_session(&mut self) {
        let BrainTab::Session(id) = self.effective_brain_tab() else {
            return;
        };
        self.close_user_session(id);
    }

    pub(crate) fn close_user_session(&mut self, id: SessionTabId) {
        if self.brain.is_receiver_session_tab(BrainTab::Session(id)) {
            return;
        }
        if self.brain.is_manual_session_tab(BrainTab::Session(id)) {
            self.close_manual_session(id);
        } else if self.brain.is_skill_session_tab(BrainTab::Session(id)) {
            self.close_skill_session(id);
        }
    }

    pub(crate) fn tick_manual_sessions(&mut self) {
        self.close_exited_brain_panel();
        for observation in self.brain.manual_session_observations() {
            let Some(outcome) = self
                .brain
                .manual_exit_decision(&observation, self.services.monotonic_now())
            else {
                continue;
            };
            match outcome {
                ExitedPanel::Close => self.close_manual_session(observation.id),
                ExitedPanel::StartupFailed => {
                    if let Err(error) = self.retain_failed_manual_startup(
                        observation.id,
                        &observation.manual_session_id,
                    ) {
                        self.report_manual_launch_error(&error);
                    } else {
                        self.status.set_error(format!(
                            "{} unavailable: exited during startup",
                            self.context.agent_kind().label()
                        ));
                    }
                }
                ExitedPanel::RetryFresh { refused } => {
                    self.brain.refuse_resume_id(refused);
                    if let Err(error) =
                        self.retry_manual_session(observation.id, &observation.manual_session_id)
                    {
                        self.brain.record_manual_restore_failed(observation.id);
                        self.report_manual_launch_error(&error);
                    }
                }
            }
        }
    }

    fn retain_failed_manual_startup(
        &mut self,
        tab_id: SessionTabId,
        manual_id: &crate::manual_session::ManualSessionId,
    ) -> anyhow::Result<()> {
        if let Some(controller) = self.brain.active_controller_mut(BrainTab::Session(tab_id)) {
            controller.shutdown()?;
        }
        self.services
            .release_manual_session(manual_id, &self.manual_session_scope())
    }

    fn retry_manual_session(
        &mut self,
        tab_id: SessionTabId,
        manual_id: &crate::manual_session::ManualSessionId,
    ) -> anyhow::Result<()> {
        if let Some(controller) = self.brain.active_controller_mut(BrainTab::Session(tab_id)) {
            controller.shutdown()?;
        }
        let record = self
            .services
            .manual_sessions(&self.manual_session_scope())?
            .into_iter()
            .find(|record| record.id == *manual_id)
            .ok_or_else(|| anyhow::anyhow!("manual session mapping was not found"))?;
        self.launch_manual_session(
            ManualLaunchTarget::Additional {
                record,
                tab_id: Some(tab_id),
            },
            None,
        )?;
        Ok(())
    }

    pub(in crate::tui) fn mark_manual_session_turn_started(&self, tab_id: SessionTabId) {
        let Some(id) = self.brain.manual_session_id(tab_id) else {
            return;
        };
        if let Err(error) =
            SessionStore::mark_active(&self.services, id.as_str(), &self.manual_session_scope())
        {
            crate::logging::log(format!("marking manual session active failed: {error:#}"));
        }
    }
}
