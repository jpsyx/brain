//! Main-panel controller construction, session authorization, and semantic turns.

use crate::tui::App;
use crate::tui::handlers::half_page_step;
use crate::tui::model::{BrainTab, Panel};

use crossterm::event::KeyCode;

use crate::agent::{AgentController, SessionStore};
use crate::pty_pane::PtyPane;

#[cfg(not(test))]
fn brain_transport(_app: &mut App) -> Box<dyn crate::agent::AgentTransport> {
    Box::new(PtyPane::new(24, 80))
}

#[cfg(test)]
fn brain_transport(app: &mut App) -> Box<dyn crate::agent::AgentTransport> {
    app.brain
        .take_brain_transport()
        .unwrap_or_else(|| Box::new(PtyPane::new(24, 80)))
}

impl App {
    pub(in crate::tui) fn launch_capability_plan(
        &self,
    ) -> anyhow::Result<Option<crate::access::CapabilityPlan>> {
        if self.context.access_mode() == crate::access::AccessMode::Unrestricted {
            return Ok(None);
        }
        let mut config = crate::config::Config::try_load(self.context.workspace())?;
        config.access_mode = self.context.access_mode();
        crate::access::capability_plan_for(&config, self.context.command())
            .map(Some)
            .map_err(anyhow::Error::from)
    }

    pub(in crate::tui) fn controller_for_transport(
        &self,
        actor: crate::actor::ActorContext,
        transport: Box<dyn crate::agent::AgentTransport>,
    ) -> AgentController {
        AgentController::configured_with_command(
            self.context.command(),
            self.context.agent_kind(),
            self.context.agent_command().to_owned(),
            actor,
            transport,
        )
    }

    /// Handle the Ctrl-N shortcut before normal key forwarding. Returning
    /// `true` tells the event loop that the chord was consumed.
    pub(crate) fn handle_new_session_shortcut(&mut self, code: KeyCode, ctrl: bool) -> bool {
        if ctrl && matches!(code, KeyCode::Char('n' | 'N')) && self.brain.any_panel_visible() {
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
        if self.brain.any_panel_visible() {
            self.status.clear_alert();
            self.shell.focus_brain();
        }
    }

    /// Switch focus to the tasks panel. On a Brain → Tasks transition we
    /// also reload tasks.csv + habits.csv: brain-driven actions (defer,
    /// remove, complete) mutate the CSVs asynchronously and we have no
    /// completion signal, so the focus switch is our cue to pick up
    /// whatever changed while the user was over in the brain panel.
    pub(crate) fn focus_tasks(&mut self) {
        let was_on_brain = self.shell.focus() == Panel::Brain;
        self.status.clear_alert();
        self.shell.focus_tasks();
        if was_on_brain {
            self.reload_after_brain();
        }
    }

    pub(crate) fn scroll_focused_half_page(&mut self, up: bool) {
        match self.shell.focus() {
            Panel::Brain => {
                if let Some(controller) = self.active_brain_controller_mut() {
                    let step = half_page_step(controller.terminal_rows().unwrap_or_default());
                    if up {
                        let _ = controller.scroll_up(step);
                    } else {
                        let _ = controller.scroll_down(step);
                    }
                }
            }
            Panel::Tasks => {
                let step = (self.tasks.tasks_per_page() / 2).max(1);
                if up {
                    self.tasks.select_prev(step);
                } else {
                    self.tasks.select_next(step);
                }
            }
        }
    }

    pub(crate) fn mark_brain_turn_started(&mut self) {
        if let Some(controller) = self.brain.main_controller() {
            let scope = crate::agent::SessionScope::new(
                controller.kind(),
                self.context.workspace().id(),
                controller.actor().clone(),
            );
            if let Err(error) =
                SessionStore::mark_active(&self.services, self.brain.instance(), &scope)
            {
                crate::logging::log(format!("marking agent session active failed: {error:#}"));
            }
        }
        if let Some(session_id) = self.receiver.interactive_completion_to_clear() {
            let path = self
                .context
                .workspace()
                .paths()
                .responses_dir()
                .join(format!("{session_id}.json"));
            let _ = std::fs::remove_file(path);
        }
        if !self.brain.turn_active() {
            crate::logging::log("brain turn started");
        }
        self.brain.mark_turn_started();
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
        self.open_or_focus_brain(Some(trimmed));
    }
}

mod session;

#[cfg(test)]
pub(super) use session::register_fresh_before_launch;
