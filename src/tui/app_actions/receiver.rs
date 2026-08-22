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
        match crate::command::server::apply_receiver_action_with(
            self.context.command(),
            crate::workspace::ReceiverAction::Toggle,
            self.receiver.intent_refresher(),
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
                self.status.set_flash(FlashKind::Info(format!(
                    "receiver {}; TUI {}; server {}; accepting {}",
                    if status.enabled {
                        "enabled"
                    } else {
                        "disabled"
                    },
                    if status.tui_live { "live" } else { "not live" },
                    if status.server_running {
                        "running"
                    } else {
                        "not running"
                    },
                    if status.accepting { "yes" } else { "no" }
                )));
            }
            Err(error) => {
                self.status.set_flash(FlashKind::Error(format!(
                    "receiver status failed: {error:#}"
                )));
            }
        }
    }
}
