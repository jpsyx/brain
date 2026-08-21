//! Persistent receiver intent and status actions shared by both TUI palettes.

use crate::tui::{App, FlashKind};

impl App {
    pub(crate) fn refresh_receiver_enabled(&mut self) {
        match crate::command::server::receiver_enabled(&self.command_context) {
            Ok(enabled) => self.receiver_enabled = enabled,
            Err(error) => crate::logging::log(format!(
                "refreshing receiver palette state failed: {error:#}"
            )),
        }
    }

    pub(crate) fn toggle_receiver(&mut self) {
        match crate::command::server::apply_receiver_action_with(
            &self.command_context,
            crate::workspace::ReceiverAction::Toggle,
            self.receiver_intent_refresher.as_ref(),
        ) {
            Ok(outcome) => {
                self.receiver_enabled = outcome.enabled();
                self.flash = Some(outcome.refresh_warning().map_or_else(
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
                self.flash = Some(FlashKind::Error(format!(
                    "receiver enablement failed: {error:#}"
                )));
            }
        }
    }

    pub(super) fn show_receiver_status(&mut self) {
        crate::logging::log("palette request receiver server status");
        match crate::command::server::read_receiver_status(&self.command_context) {
            Ok(status) => {
                self.receiver_enabled = status.enabled;
                self.flash = Some(FlashKind::Info(format!(
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
                self.flash = Some(FlashKind::Error(format!(
                    "receiver status failed: {error:#}"
                )));
            }
        }
    }
}
