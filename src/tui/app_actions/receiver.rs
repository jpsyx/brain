//! Persistent receiver intent and status actions shared by both TUI palettes.

use crate::tui::{App, FlashKind};

impl App<'_> {
    pub(crate) fn refresh_receiver_enabled(&mut self) {
        match crate::command::server::receiver_enabled(&self.command_context) {
            Ok(enabled) => self.receiver_enabled = enabled,
            Err(error) => crate::logging::log(format!(
                "refreshing receiver palette state failed: {error:#}"
            )),
        }
    }

    pub(crate) fn toggle_receiver(&mut self) {
        match crate::command::server::apply_receiver_action(
            &self.command_context,
            crate::workspace::ReceiverAction::Toggle,
        ) {
            Ok(enabled) => {
                self.receiver_enabled = enabled;
                self.flash = Some(FlashKind::Info(format!(
                    "receiver {}",
                    if enabled { "enabled" } else { "disabled" }
                )));
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
        self.refresh_receiver_enabled();
        let client = crate::server::control::ServerClient::default();
        let server_running = client.connect_existing().is_ok();
        let tui_live = server_running
            && client
                .workspace_ingress(self.command_context.workspace.id())
                .is_ok();
        self.flash = Some(FlashKind::Info(format!(
            "receiver {}; TUI {}; server {}; accepting {}",
            if self.receiver_enabled {
                "enabled"
            } else {
                "disabled"
            },
            if tui_live { "live" } else { "not live" },
            if server_running {
                "running"
            } else {
                "not running"
            },
            if self.receiver_enabled && tui_live {
                "yes"
            } else {
                "no"
            }
        )));
    }
}
