//! Accepted jobs outlive modal generations; every result settles its own entry.
use super::worker::{NativeService, RemovalService, Request, Response};
use crate::{
    tui::task_removal::{RemovalScreen, RemovalState},
    workspace::CommandContext,
};
use std::collections::BTreeMap;

pub(crate) struct Settlement {
    pub(crate) state: Option<RemovalState>,
    pub(crate) revision: u64,
    pub(crate) preallocated_apply: bool,
    accepted: bool,
}

pub(crate) struct Coordinator {
    service: Box<dyn RemovalService>,
    next_id: u64,
    completions: BTreeMap<u64, crate::tasks::identity::TaskUuid>,
    jobs: BTreeMap<u64, Settlement>,
    refresh_pending: Option<u64>,
    recovery: BTreeMap<u64, RemovalState>,
}
impl Default for Coordinator {
    fn default() -> Self {
        Self {
            service: Box::<NativeService>::default(),
            next_id: 0,
            completions: BTreeMap::new(),
            jobs: BTreeMap::new(),
            refresh_pending: None,
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
        self.jobs.values().filter(|job| job.accepted).count()
    }
    pub(crate) fn recoverable(&self) -> Option<&RemovalState> {
        self.recovery.values().next()
    }
    pub(crate) fn start_completion(
        &mut self,
        context: CommandContext,
        request: Request,
        revision: u64,
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
        self.jobs.insert(
            id,
            Settlement {
                state: None,
                revision,
                preallocated_apply: false,
                accepted: true,
            },
        );
        self.service.start(context, id, request);
        Some(id)
    }
    pub(crate) fn start(
        &mut self,
        context: CommandContext,
        mut state: RemovalState,
        request: Request,
        revision: u64,
    ) -> RemovalState {
        self.next_id += 1;
        state.generation = self.next_id;
        let accepted = !matches!(
            request,
            Request::Preview { .. } | Request::Status(_) | Request::Refresh
        );
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
        self.jobs.insert(
            state.generation,
            Settlement {
                state: Some(state.clone()),
                revision,
                preallocated_apply: matches!(request, Request::Apply { .. }),
                accepted,
            },
        );
        self.service.start(context, state.generation, request);
        state
    }
    pub(crate) fn refresh(&mut self, context: CommandContext, revision: u64) {
        if self.refresh_pending.is_some() {
            return;
        }
        self.next_id += 1;
        let id = self.next_id;
        self.refresh_pending = Some(id);
        self.jobs.insert(
            id,
            Settlement {
                state: None,
                revision,
                preallocated_apply: false,
                accepted: false,
            },
        );
        self.service.start(context, id, Request::Refresh);
    }
    pub(crate) fn poll(&mut self) -> Option<(u64, Settlement, Response)> {
        while let Some((id, response)) = self.service.poll() {
            self.completions.remove(&id);
            if self.refresh_pending == Some(id) {
                self.refresh_pending = None;
            }
            if let Some(job) = self.jobs.remove(&id) {
                return Some((id, job, response));
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
