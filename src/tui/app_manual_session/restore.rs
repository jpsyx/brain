use crate::manual_session::ManualSessionRole;
use crate::tui::App;

use super::ManualLaunchTarget;

impl App {
    pub(crate) fn restore_manual_sessions(&mut self) {
        let Some(mut records) = self.brain.take_manual_sessions_to_restore() else {
            return;
        };
        if !self
            .brain
            .main_controller()
            .is_some_and(|controller| controller.is_alive().unwrap_or(false))
            && let Err(error) = self.launch_manual_session(ManualLaunchTarget::Main, None)
        {
            self.report_manual_launch_error(&error);
        }
        records.sort_by_key(|record| record.position);
        for record in records {
            if record.role == ManualSessionRole::Main
                || self
                    .brain
                    .manual_session_observations()
                    .iter()
                    .any(|tab| tab.manual_session_id == record.id)
            {
                continue;
            }
            if let Err(error) = self.launch_manual_session(
                ManualLaunchTarget::Additional {
                    record,
                    tab_id: None,
                },
                None,
            ) {
                self.report_manual_launch_error(&error);
            }
        }
    }

    pub(crate) fn release_manual_session_locks(&self) -> anyhow::Result<()> {
        let scope = self.manual_session_scope();
        let records = self.services.manual_sessions(&scope)?;
        let mut errors = Vec::new();
        for record in records {
            if let Err(error) = self.services.release_manual_session(&record.id, &scope) {
                errors.push(error.to_string());
            }
        }
        anyhow::ensure!(
            errors.is_empty(),
            "manual session lock release failed: {}",
            errors.join("; ")
        );
        Ok(())
    }
}
