//! Main-panel shutdown, ticking, and reload lifecycle.

use crate::tui::App;
use crate::tui::modal_state::FlashKind;

use crate::agent::SessionStore;
use crate::tui::app_brain::launch::arrival::{ExitedPanel, decide_exited_panel};

impl App {
    /// Close the brain panel: explicitly shut down its `AgentController`,
    /// release the session lock so a later open (or another shell) can resume
    /// it via recency, hand the screen back to full-width tasks, and reload so
    /// a brain action whose effect landed right before the close shows up
    /// immediately.
    pub(crate) fn close_brain(&mut self) {
        if let Some(mut controller) = self.brain.take_main() {
            let _ = controller.shutdown();
        }
        let scope = crate::agent::SessionScope::new(
            self.context.agent_kind(),
            self.context.workspace().id(),
            crate::actor::ActorContext::follow_up(self.brain.interactive_actor()),
        );
        if let Some(session_id) = self
            .services
            .locked_session_for_instance(self.brain.instance(), &scope)
        {
            self.brain.record_interactive_agent_session(session_id);
        }
        self.brain.clear_session();
        self.brain.disarm_resume_arrival();
        self.status.clear_alert();
        self.shell.focus_tasks();
        let _ = SessionStore::release(&self.services, self.brain.instance());
        self.reload_after_brain();
    }

    /// Handle a brain panel whose agent has exited. A resume the frontend
    /// refused quits on arrival, which would otherwise leave the user a dead
    /// panel and nothing to message; that id is retired for the run and a fresh
    /// session opens in its place. Returns whether the panel needed handling.
    pub(crate) fn close_exited_brain_panel(&mut self) -> bool {
        if !self
            .brain
            .main_controller()
            .is_some_and(|controller| controller.is_alive().is_ok_and(|alive| !alive))
        {
            return false;
        }
        let outcome = decide_exited_panel(self.brain.resume_arrival());
        self.close_brain();
        if let ExitedPanel::RetryFresh { refused } = outcome {
            self.brain.refuse_resume_id(refused.clone());
            if !self.brain.claim_resume_retry() {
                return true;
            }
            crate::logging::log(format!(
                "brain panel resume refused on arrival session={refused}; opening a fresh session"
            ));
            self.open_or_focus_brain(None);
            self.status.set_alert(Some(
                "⚠ couldn't resume your last conversation; started a new brain chat".to_owned(),
            ));
        }
        true
    }

    /// End every live agent child before the owning shell drops its transports.
    pub(crate) fn shutdown_agent_controllers(&mut self) -> Vec<crate::agent::AgentError> {
        self.brain.shutdown_controllers()
    }

    /// Re-read the CSVs after a brain interaction; route any error to
    /// the flash line so a transient load failure doesn't block the
    /// focus switch the user actually asked for.
    pub(crate) fn reload_after_brain(&mut self) {
        if let Err(e) = self.reload_tasks() {
            self.status
                .set_flash(FlashKind::Error(format!("⚠ reload failed: {e}")));
        }
    }

    /// User-triggered refresh (the `r` hotkey). Re-reads the CSVs and
    /// flashes a confirmation so the user sees that the repaint
    /// actually happened, even when nothing visible changed.
    pub(crate) fn refresh(&mut self) {
        crate::tui::runtime::tick::refresh(self);
    }
}
