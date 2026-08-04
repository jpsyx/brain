//! Main-panel shutdown, completion fallback, ticking, and reload lifecycle.

use crate::tui::*;

use crate::agent::SessionStore;

impl App<'_> {
    /// Close the brain panel: drop the PTY (its Drop impl kills the agent
    /// child, ending the session process), release the session lock so a
    /// later open (or another shell) can resume it via recency, hand the
    /// screen back to full-width tasks, and reload so a brain action whose
    /// effect landed right before the close shows up immediately.
    pub(crate) fn close_brain(&mut self) {
        self.close_brain_with(Self::deliver_completed_remote_turn);
    }

    pub(super) fn close_brain_with(
        &mut self,
        deliver: impl FnOnce(&mut Self, crate::server::delivery::CompletionDelivery),
    ) {
        let receiver_panel = self.receiver_session_id.is_some();
        let completed_remote = self.brain.as_ref().is_some_and(|panel| !panel.is_alive())
            && receiver_panel
            && self.receiver_started.is_some();
        let completion = completed_remote
            .then_some(self.brain.as_ref())
            .flatten()
            .and_then(crate::server::delivery::CompletionDelivery::capture);
        if let Some(completion) = completion {
            deliver(self, completion);
        }
        if let Some(mut controller) = self.brain.take() {
            controller.shutdown();
        }
        self.session_actor = None;
        self.brain_turn_active = false;
        self.alert = None;
        self.focus = Panel::Tasks;
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
        let (snapshot, actor, channel) = completion.into_parts();
        let Some(sender) = self.receiver_sender.clone() else {
            return;
        };
        match channel {
            crate::server::receiver::Channel::Sms => {
                let reply = crate::server::reply::sms(&snapshot);
                crate::server::delivery::send_sms_background(
                    self.command_context.clone(),
                    "fallback final SMS response",
                    sender,
                    reply.text,
                );
            }
            crate::server::receiver::Channel::Email => {
                let recipients = self.receiver_email_recipients(&self.receiver_recipients, &actor);
                if !recipients.is_empty() {
                    let reply = crate::server::reply::email(&snapshot);
                    let html = crate::server::reply::email_html(&reply.text);
                    crate::server::delivery::send_email_background(
                        self.command_context.clone(),
                        "fallback final email response",
                        recipients,
                        "Brain response".to_owned(),
                        reply.text,
                        html,
                    );
                }
            }
        }
    }

    pub(crate) fn close_exited_brain_panel(&mut self) -> bool {
        if self
            .brain
            .as_ref()
            .is_some_and(|controller| !controller.is_alive())
        {
            self.close_brain();
            return true;
        }
        false
    }

    /// Advance frontend-neutral delayed controller input for live panels.
    pub(crate) fn tick_agent_controllers(&mut self) {
        for controller in [&mut self.brain, &mut self.triage_brain]
            .into_iter()
            .flatten()
        {
            if let Err(error) = controller.tick() {
                crate::logging::log(format!("agent input delivery failed: {error}"));
            }
        }
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
        // A tasks session can span days. Advance `today` first if this refresh
        // crossed into a new logical day (6 AM rollover by default) so the
        // rebuilt view uses the new date, then re-open the daily-triage nudge
        // against the freshly-reloaded habits.
        let rolled = self.advance_triage_day(chrono::Local::now().naive_local());
        let reload = self.reload_tasks();
        if rolled {
            self.check_daily_triage();
        }
        self.flash = Some(match reload {
            Ok(()) => FlashKind::Info("✓ refreshed".to_string()),
            Err(e) => FlashKind::Error(format!("⚠ reload failed: {e}")),
        });
    }
}
