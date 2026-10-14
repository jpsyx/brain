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
    pub(crate) fn recoverable_removal(&self) -> Option<&RemovalState> {
        self.removal.recoverable()
    }
    pub(crate) fn start_removal(
        &mut self,
        context: CommandContext,
        state: RemovalState,
        request: Request,
    ) -> RemovalState {
        self.removal.start(context, state, request)
    }
    pub(crate) fn poll_removal(&mut self) -> Option<(RemovalState, Response)> {
        self.removal.poll()
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
