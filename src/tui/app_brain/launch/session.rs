use crate::tui::App;
use crate::tui::app_manual_session::ManualLaunchTarget;
use crate::tui::model::BrainTab;

impl App {
    /// Select Main and optionally queue a prompt, launching its controller when needed.
    pub(crate) fn open_or_focus_brain(&mut self, prompt: Option<&str>) -> bool {
        if self
            .brain
            .main_controller()
            .is_some_and(|controller| controller.is_alive().unwrap_or(false))
        {
            self.select_brain_tab(BrainTab::Main);
            if let Some(prompt) = prompt {
                if let Some(controller) = self.brain.main_controller_mut()
                    && let Err(error) = controller.queue_after_active_turn(prompt)
                {
                    crate::logging::log(format!("brain prompt queue failed: {error}"));
                    return false;
                }
                self.mark_brain_turn_started();
            }
            return true;
        }
        if self.brain.main_controller().is_some() && !self.stop_main_controller() {
            return false;
        }
        self.brain.begin_interactive_session_launch();
        match self.launch_manual_session(ManualLaunchTarget::Main, prompt) {
            Ok(tab) => {
                // The launch's resume warning survives selecting Main.
                let open = self.brain.session_tab_ids();
                self.shell.select_brain_tab(tab, &open, true);
                true
            }
            Err(error) => {
                self.brain.record_interactive_session_launch_failed();
                self.brain.clear_session();
                self.report_manual_launch_error(&error);
                false
            }
        }
    }
}
