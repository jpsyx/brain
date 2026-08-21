//! Main-panel shutdown, completion fallback, ticking, and reload lifecycle.

use crate::tui::*;

use crate::agent::SessionStore;

impl App {
    /// Close the brain panel: explicitly shut down its `AgentController`,
    /// release the session lock so a later open (or another shell) can resume
    /// it via recency, hand the screen back to full-width tasks, and reload so
    /// a brain action whose effect landed right before the close shows up
    /// immediately.
    pub(crate) fn close_brain(&mut self) {
        self.close_brain_with(Self::deliver_completed_remote_turn);
    }

    pub(super) fn close_brain_with(
        &mut self,
        deliver: impl FnOnce(&mut Self, crate::server::delivery::CompletionDelivery),
    ) {
        let receiver_panel = self.receiver.has_receiver_session();
        let completed_remote = self
            .brain
            .as_ref()
            .is_some_and(|panel| panel.is_alive().is_ok_and(|alive| !alive))
            && receiver_panel
            && self.receiver.remote_turn_in_flight();
        let completion = completed_remote
            .then_some(self.brain.as_ref())
            .flatten()
            .and_then(crate::server::delivery::CompletionDelivery::capture);
        if let Some(completion) = completion {
            deliver(self, completion);
        }
        if let Some(mut controller) = self.brain.take() {
            let _ = controller.shutdown();
        }
        if !receiver_panel {
            let scope = crate::agent::SessionScope::new(
                self.agent_kind,
                self.command_context.workspace.id(),
                crate::actor::ActorContext::follow_up(&self.interactive_actor),
            );
            if let Some(session_id) = self.db.locked_session_for_instance(&self.instance, &scope) {
                self.receiver.record_interactive_agent_session(session_id);
            }
        }
        self.session_actor = None;
        self.brain_turn_active = false;
        self.alert = None;
        self.shell.focus_tasks();
        let _ = SessionStore::release(&self.db, &self.instance);
        if receiver_panel {
            self.clear_receiver_panel_state();
        }
        self.reload_after_brain();
    }

    fn deliver_completed_remote_turn(
        &mut self,
        completion: crate::server::delivery::CompletionDelivery,
    ) {
        let (snapshot, _actor, channel) = completion.into_parts();
        let Some(target) = self.receiver.active_delivery_target() else {
            return;
        };
        match channel {
            crate::server::receiver::Channel::Sms => {
                let reply = crate::server::reply::sms(&snapshot);
                crate::server::delivery::send_sms_background(
                    self.command_context.clone(),
                    "fallback final SMS response",
                    target.sender,
                    reply.text,
                );
            }
            crate::server::receiver::Channel::Email => {
                self.send_email_reply("fallback final email response", &snapshot);
            }
        }
    }

    pub(crate) fn close_exited_brain_panel(&mut self) -> bool {
        if self
            .brain
            .as_ref()
            .is_some_and(|controller| controller.is_alive().is_ok_and(|alive| !alive))
        {
            self.close_brain();
            return true;
        }
        false
    }

    /// End every live agent child before the owning shell drops its transports.
    pub(crate) fn shutdown_agent_controllers(&mut self) -> Vec<crate::agent::AgentError> {
        let mut errors = Vec::new();
        for controller in self.brain.iter_mut().chain(
            self.skill_sessions
                .iter_mut()
                .map(|tab| &mut tab.controller),
        ) {
            if let Err(error) = controller.shutdown() {
                errors.push(error);
            }
        }
        errors
    }

    /// Re-read the CSVs after a brain interaction; route any error to
    /// the flash line so a transient load failure doesn't block the
    /// focus switch the user actually asked for.
    pub(crate) fn reload_after_brain(&mut self) {
        if let Err(e) = self.reload_tasks() {
            self.flash = Some(FlashKind::Error(format!("⚠ reload failed: {e}")));
        }
    }

    /// User-triggered refresh (the `r` hotkey). Re-reads the CSVs and
    /// flashes a confirmation so the user sees that the repaint
    /// actually happened, even when nothing visible changed.
    pub(crate) fn refresh(&mut self) {
        crate::tui::runtime::tick::refresh(self);
    }
}
