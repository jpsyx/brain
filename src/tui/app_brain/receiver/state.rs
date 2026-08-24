//! Receiver panel ownership and warm-channel state.

use crate::tui::App;

use crate::agent::SessionStore;

impl App {
    pub(in crate::tui::app_brain) fn close_receiver_panel(&mut self, restore_interactive: bool) {
        let resume_session = self
            .receiver
            .interactive_agent_session_to_resume()
            .and_then(|id| crate::agent::AgentSession::new(id).ok());
        let can_resume = self.brain.main_controller().is_some_and(|controller| {
            resume_session.as_ref().is_some_and(|session| {
                controller
                    .can_resume_response_session(session)
                    .unwrap_or(false)
            })
        });
        if let Some(mut controller) = self.brain.take_main() {
            let _ = controller.shutdown();
        }
        self.brain.clear_session();
        self.status.clear_alert();
        self.shell.focus_tasks();
        let _ = SessionStore::release(&self.services, self.brain.instance());
        self.clear_receiver_panel_state();
        self.reload_after_brain();
        if restore_interactive {
            self.receiver.prepare_interactive_restore(can_resume);
            self.open_or_focus_brain(None);
        }
    }

    pub(in crate::tui::app_brain) fn clear_receiver_panel_state(&mut self) {
        self.receiver.clear_receiver_panel_state();
    }
}
