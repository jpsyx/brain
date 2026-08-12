//! Receiver panel ownership and warm-channel state.

use crate::tui::*;

use crate::agent::SessionStore;

impl App<'_> {
    pub(in crate::tui::app_brain) fn close_receiver_panel(&mut self, restore_interactive: bool) {
        let resume_session = self
            .interactive_agent_session_id
            .as_deref()
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
        self.focus = Panel::Tasks;
        let _ = SessionStore::release(&self.db, &self.instance);
        self.clear_receiver_panel_state();
        self.reload_after_brain();
        if restore_interactive {
            self.receiver_resume_session = can_resume
                .then(|| self.interactive_agent_session_id.take())
                .flatten();
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
        self.receiver_session_id.is_some() && self.receiver_started.is_none()
    }

    pub(in crate::tui::app_brain) fn clear_receiver_panel_state(&mut self) {
        self.receiver_sender = None;
        self.receiver_recipients.clear();
        self.receiver_response_email = None;
        self.receiver_email_reply = None;
        self.receiver_session_id = None;
        self.receiver_lease = None;
        self.receiver_started = None;
        self.receiver_delay_sent = false;
        self.receiver_probe = None;
        self.receiver_panel_activity = None;
        self.requested_receiver_actor = None;
    }
}
