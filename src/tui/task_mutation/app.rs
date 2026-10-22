use super::{projection::Change, worker::Request};
use crate::{
    tasks::removal::model::Identity,
    tui::{App, modal_state::FlashKind},
};
impl App {
    pub(crate) fn queue_completion(&mut self, identity: Identity, today: chrono::NaiveDate) {
        self.tasks.bind_workspace(self.context.workspace().id());
        let Some(job) = self.services.start_task_completion(
            self.context.command().clone(),
            Request::Complete {
                identity: identity.clone(),
                today,
            },
        ) else {
            return;
        };
        self.tasks
            .project_task_change(job, vec![identity], Change::Complete(today));
    }
    pub(crate) fn task_change_error(&mut self, message: String) {
        let message = if let Some(previous) = self.status.error() {
            format!("{previous}\n{message}")
        } else {
            message
        };
        self.status.set_error(message);
    }
    pub(crate) fn task_changes_allow_quit(&mut self) -> bool {
        if self.services.pending_task_changes() == 0 {
            return true;
        }
        self.status.set_flash(FlashKind::Info(
            "Task changes are still saving. Please wait before quitting.".into(),
        ));
        false
    }
}
