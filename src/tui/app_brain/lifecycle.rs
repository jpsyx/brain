//! Main-panel shutdown, ticking, and reload lifecycle.

use crate::tui::App;
use crate::tui::modal_state::FlashKind;

use crate::tui::app_brain::launch::arrival::ExitedPanel;

impl App {
    pub(in crate::tui) fn stop_main_controller(&mut self) -> bool {
        if let Some(controller) = self.brain.main_controller_mut()
            && let Err(error) = controller.shutdown()
        {
            self.report_manual_launch_error(&error.into());
            return false;
        }
        let scope = self.manual_session_scope();
        if let Some(session_id) = self
            .services
            .locked_session_for_instance(self.brain.instance(), &scope)
        {
            self.brain.record_interactive_agent_session(session_id);
        }
        if let Err(error) = self
            .services
            .release_manual_session(self.brain.main_manual_session_id(), &scope)
        {
            crate::logging::log(format!("main session release failed: {error:#}"));
        }
        self.brain.take_main();
        self.brain.clear_session();
        self.brain.disarm_resume_arrival();
        self.reload_after_brain();
        true
    }

    /// Relaunch an exited Main controller without changing tab selection or focus.
    pub(crate) fn close_exited_brain_panel(&mut self) -> bool {
        let Some(alive) = self
            .brain
            .main_controller()
            .and_then(|controller| controller.is_alive().ok())
        else {
            return false;
        };
        let Some(outcome) = self
            .brain
            .main_exit_decision(alive, self.services.monotonic_now())
        else {
            return false;
        };
        if !self.stop_main_controller() {
            return true;
        }
        if outcome == ExitedPanel::StartupFailed {
            self.status.set_alert(Some(format!(
                "{} unavailable: exited during startup",
                self.context.agent_kind().label()
            )));
            return true;
        }
        let refused = if let ExitedPanel::RetryFresh { refused } = outcome {
            self.brain.refuse_resume_id(refused);
            true
        } else {
            false
        };
        if let Err(error) = self.launch_manual_session(
            crate::tui::app_manual_session::ManualLaunchTarget::Main,
            None,
        ) {
            self.report_manual_launch_error(&error);
        } else if refused {
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
