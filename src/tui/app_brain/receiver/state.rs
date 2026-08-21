//! Receiver panel ownership and warm-channel state.

use crate::tui::*;

use crate::agent::SessionStore;

impl App {
    pub(super) fn close_idle_panel_for_receiver_dispatch(&mut self, receiver_panel: bool) {
        if receiver_panel {
            crate::logging::log("receiver dispatch switching from a warm receiver channel");
            self.close_receiver_panel(false);
        } else {
            crate::logging::log("receiver dispatch replacing idle interactive brain panel");
            self.close_brain();
        }
    }

    pub(in crate::tui::app_brain) fn close_receiver_panel(&mut self, restore_interactive: bool) {
        let resume_session = self
            .receiver
            .interactive_agent_session_to_resume()
            .and_then(|id| crate::agent::AgentSession::new(id).ok());
        let can_resume = self.brain.as_ref().is_some_and(|controller| {
            resume_session.as_ref().is_some_and(|session| {
                controller
                    .can_resume_response_session(session)
                    .unwrap_or(false)
            })
        });
        if let Some(mut controller) = self.brain.take() {
            let _ = controller.shutdown();
        }
        self.session_actor = None;
        self.brain_turn_active = false;
        self.alert = None;
        self.shell.focus_tasks();
        let _ = SessionStore::release(&self.db, &self.instance);
        self.clear_receiver_panel_state();
        self.reload_after_brain();
        if restore_interactive {
            self.receiver.prepare_interactive_restore(can_resume);
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

    pub(in crate::tui::app_brain) fn receiver_panel_is_warm(&self) -> bool {
        self.receiver.receiver_panel_is_warm()
    }

    pub(in crate::tui::app_brain) fn clear_receiver_panel_state(&mut self) {
        self.receiver.clear_receiver_panel_state();
    }
}
