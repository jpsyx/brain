//! Injected native removal runtime, independent of the modal and frontend.
use super::AppServices;
use crate::{
    tui::task_removal::{
        RemovalState,
        worker::{Request, Response},
    },
    workspace::CommandContext,
};
impl AppServices {
    pub(crate) fn pending_task_changes(&self) -> usize {
        self.removal.pending()
    }
    pub(crate) fn start_task_completion(
        &mut self,
        context: CommandContext,
        request: Request,
        revision: u64,
    ) -> Option<u64> {
        self.removal.start_completion(context, request, revision)
    }
    pub(crate) fn recoverable_removal(&self) -> Option<&RemovalState> {
        self.removal.recoverable()
    }
    pub(crate) fn start_removal(
        &mut self,
        context: CommandContext,
        state: RemovalState,
        request: Request,
        revision: u64,
    ) -> RemovalState {
        self.removal.start(context, state, request, revision)
    }
    pub(crate) fn poll_removal(
        &mut self,
    ) -> Option<(u64, crate::tui::task_mutation::Settlement, Response)> {
        self.removal.poll()
    }
    pub(crate) fn refresh_task_snapshot(&mut self, context: CommandContext, revision: u64) {
        self.removal.refresh(context, revision);
    }
    pub(crate) fn remember_removal(&mut self, state: RemovalState) {
        self.removal.remember(state);
    }
    #[cfg(test)]
    pub(crate) fn replace_removal_service(
        &mut self,
        service: Box<dyn crate::tui::task_removal::worker::RemovalService>,
    ) {
        self.removal.replace_service(service);
    }
}
