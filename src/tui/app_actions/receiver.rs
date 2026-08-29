//! Persistent receiver intent and status actions shared by both TUI palettes.

use crate::tui::App;
use crate::tui::modal_state::FlashKind;

impl App {
    pub(crate) fn refresh_receiver_enabled(&mut self) {
        match crate::command::server::receiver_enabled(self.context.command()) {
            Ok(enabled) => self.receiver.record_intent(enabled),
            Err(error) => crate::logging::log(format!(
                "refreshing receiver palette state failed: {error:#}"
            )),
        }
    }

    pub(crate) fn toggle_receiver(&mut self) {
        match self.services.apply_receiver_action(
            self.context.command(),
            crate::workspace::ReceiverAction::Toggle,
        ) {
            Ok(outcome) => {
                self.receiver.record_intent(outcome.enabled());
                self.status.set_flash(outcome.refresh_warning().map_or_else(
                    || {
                        FlashKind::Info(format!(
                            "receiver {}",
                            if outcome.enabled() {
                                "enabled"
                            } else {
                                "disabled"
                            }
                        ))
                    },
                    |warning| {
                        FlashKind::Error(format!(
                            "receiver {}; warning: {warning}",
                            if outcome.enabled() {
                                "enabled"
                            } else {
                                "disabled"
                            }
                        ))
                    },
                ));
            }
            Err(error) => {
                self.status.set_flash(FlashKind::Error(format!(
                    "receiver enablement failed: {error:#}"
                )));
            }
        }
    }

    pub(super) fn show_receiver_status(&mut self) {
        crate::logging::log("palette request receiver server status");
        match crate::command::server::read_receiver_status(self.context.command()) {
            Ok(status) => {
                self.receiver.record_intent(status.enabled);
                let work = crate::command::server::read_work_state(self.context.command());
                self.status.set_flash(FlashKind::Info(
                    crate::command::server::receiver_status_flash(
                        status,
                        &work,
                        crate::theme::Theme::dark(false),
                    ),
                ));
            }
            Err(error) => {
                self.status.set_flash(FlashKind::Error(format!(
                    "receiver status failed: {error:#}"
                )));
            }
        }
    }
}
