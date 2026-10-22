//! Accepted jobs outlive modal generations; every result settles its own entry.
use super::worker::{NativeService, RemovalService, Request, Response};
use crate::{
    tui::task_removal::{RemovalScreen, RemovalState},
    workspace::CommandContext,
};
use std::collections::BTreeMap;

pub(crate) struct Coordinator {
    service: Box<dyn RemovalService>,
    next_id: u64,
    completions: BTreeMap<u64, crate::tasks::identity::TaskUuid>,
    jobs: BTreeMap<u64, (Option<RemovalState>, bool)>,
    recovery: BTreeMap<u64, RemovalState>,
}
impl Default for Coordinator {
    fn default() -> Self {
        Self {
            service: Box::<NativeService>::default(),
            next_id: 0,
            completions: BTreeMap::new(),
            jobs: BTreeMap::new(),
            recovery: BTreeMap::new(),
        }
    }
}
impl Coordinator {
    #[cfg(test)]
    pub(crate) fn replace_service(&mut self, service: Box<dyn RemovalService>) {
        self.service = service;
    }
    pub(crate) fn pending(&self) -> usize {
        self.jobs.values().filter(|(_, accepted)| *accepted).count()
    }
    pub(crate) fn recoverable(&self) -> Option<&RemovalState> {
        self.recovery.values().next()
    }
    pub(crate) fn start_completion(
        &mut self,
        context: CommandContext,
        request: Request,
    ) -> Option<u64> {
        let Request::Complete { identity, .. } = &request else {
            return None;
        };
        if self
            .completions
            .values()
            .any(|uuid| *uuid == identity.task_uuid)
        {
            return None;
        }
        self.next_id += 1;
        let id = self.next_id;
        self.completions.insert(id, identity.task_uuid);
        self.jobs.insert(id, (None, true));
        self.service.start(context, id, request);
        Some(id)
    }
    pub(crate) fn start(
        &mut self,
        context: CommandContext,
        mut state: RemovalState,
        request: Request,
    ) -> RemovalState {
        self.next_id += 1;
        state.generation = self.next_id;
        let accepted = !matches!(request, Request::Preview { .. } | Request::Status(_));
        if let Request::Apply { operation_id, .. } = &request {
            state.operation_id = Some(operation_id.clone());
        }
        if let Some(id) = &state.operation_id {
            self.recovery
                .retain(|_, previous| previous.operation_id.as_ref() != Some(id));
        }
        state.screen = RemovalScreen::Running {
            operation_id: state.operation_id.clone(),
        };
        state.abandon_confirm = false;
        state.scroll = 0;
        state.selected = 0;
        self.jobs
            .insert(state.generation, (Some(state.clone()), accepted));
        self.service.start(context, state.generation, request);
        state
    }
    pub(crate) fn poll(&mut self) -> Option<(u64, Option<RemovalState>, Response)> {
        while let Some((id, response)) = self.service.poll() {
            self.completions.remove(&id);
            if let Some((state, _)) = self.jobs.remove(&id) {
                return Some((id, state, response));
            }
        }
        None
    }
    pub(crate) fn remember(&mut self, state: RemovalState) {
        if let Some(id) = &state.operation_id {
            self.recovery
                .retain(|_, previous| previous.operation_id.as_ref() != Some(id));
        }
        if state.operation_id.is_some() {
            self.recovery.insert(state.generation, state);
        }
    }
}
