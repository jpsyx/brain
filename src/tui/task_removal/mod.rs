//! Removal display and request coordination; workers outlive their modal.
mod actions;
mod effects;
mod input;
mod render;
mod state;
pub(crate) mod worker;
use crate::tasks::removal::model::Options;
use crate::tui::{
    App,
    modal_state::FlashKind,
    overlay::{Overlay, close_overlay, replace_overlay},
};
pub(crate) use input::handle_key;
pub(crate) use render::draw;
pub(crate) use state::{RemovalScreen, RemovalState};
use worker::{RemovalService, Request, Response};

pub(crate) struct Coordinator {
    service: Box<dyn RemovalService>,
    next_generation: u64,
    state: Option<RemovalState>,
    executing: bool,
}
impl Default for Coordinator {
    fn default() -> Self {
        Self {
            service: Box::<worker::NativeService>::default(),
            next_generation: 0,
            state: None,
            executing: false,
        }
    }
}
impl Coordinator {
    #[cfg(test)]
    pub(crate) fn replace_service(&mut self, service: Box<dyn RemovalService>) {
        self.service = service;
    }
    pub(crate) fn recoverable(&self) -> Option<&RemovalState> {
        self.state
            .as_ref()
            .filter(|state| self.executing || state.operation_id.is_some())
    }
    pub(crate) fn start(
        &mut self,
        context: crate::workspace::CommandContext,
        mut state: RemovalState,
        request: Request,
    ) -> RemovalState {
        self.next_generation += 1;
        state.generation = self.next_generation;
        state.screen = RemovalScreen::Running {
            operation_id: state.operation_id.clone(),
        };
        state.abandon_confirm = false;
        state.scroll = 0;
        state.selected = 0;
        self.executing = !matches!(request, Request::Preview { .. });
        self.service.start(context, state.generation, request);
        self.state = Some(state.clone());
        state
    }
    pub(crate) fn poll(&mut self) -> Option<(RemovalState, Response)> {
        while let Some((generation, response)) = self.service.poll() {
            if let Some(state) = self
                .state
                .as_ref()
                .filter(|state| state.generation == generation)
            {
                self.executing = false;
                return Some((state.clone(), response));
            }
        }
        None
    }
    pub(crate) fn remember(&mut self, state: RemovalState) {
        self.state = Some(state);
    }
}
impl App {
    pub(crate) fn confirm_removal(&mut self) -> bool {
        let Some(Overlay::TaskRemoval(state)) = &self.overlay else {
            return false;
        };
        if !actions::choices(state)
            .iter()
            .any(|(action, _)| *action == actions::Action::Delete)
        {
            return true;
        }
        if let RemovalScreen::Preview(plan) = &state.screen {
            let request = Request::Apply {
                plan: Box::new(plan.clone()),
                expected: plan.fingerprint.clone(),
                today: chrono::Local::now().date_naive(),
            };
            self.removal_request(state.clone(), request);
        }
        true
    }

    pub(crate) fn run_remove(&mut self, raw_id: &str) {
        if self.reopen_removal() {
            return;
        }
        let state = RemovalState {
            generation: 0,
            targets: vec![raw_id.into()],
            options: Options::default(),
            screen: RemovalScreen::Running { operation_id: None },
            operation_id: None,
            notice: String::new(),
            abandon_confirm: false,
            scroll: 0,
            selected: 0,
        };
        self.removal_preview(state);
    }
    pub(super) fn removal_preview(&mut self, state: RemovalState) {
        let request = Request::Preview {
            targets: state.targets.clone(),
            options: state.options.clone(),
            today: chrono::Local::now().date_naive(),
        };
        self.removal_request(state, request);
    }
    pub(crate) fn reopen_removal(&mut self) -> bool {
        if let Some(state) = self.services.recoverable_removal() {
            replace_overlay(&mut self.overlay, Overlay::TaskRemoval(state.clone()));
            true
        } else {
            false
        }
    }
    pub(super) fn removal_request(&mut self, state: RemovalState, request: Request) {
        let state = self
            .services
            .start_removal(self.context.command().clone(), state, request);
        replace_overlay(&mut self.overlay, Overlay::TaskRemoval(state));
    }
    pub(crate) fn tick_removal(&mut self) {
        while let Some((mut state, response)) = self.services.poll_removal() {
            let generation = state.generation;
            let visible = matches!(&self.overlay, Some(Overlay::TaskRemoval(modal)) if modal.generation == generation);
            let outcome = match response {
                Response::Preview(result) => result.map(RemovalScreen::Preview),
                Response::Receipt(result) => result.map(RemovalScreen::Finished),
                Response::Operation(result) => result.map(|operation| {
                    state.operation_id = Some(operation.operation_id.clone());
                    if let Some(receipt) = operation.receipt {
                        RemovalScreen::Finished(receipt)
                    } else {
                        if operation.phase == crate::tasks::removal::operations::Phase::Abandoned {
                            state.operation_id = None;
                        }
                        RemovalScreen::Failed(crate::tasks::removal::model::Problem::new(
                            "operation_status",
                            match operation.phase {
                                crate::tasks::removal::operations::Phase::Prepared => "Deletion is prepared. Retry to continue.",
                                crate::tasks::removal::operations::Phase::ExternalPending => "Linked issues still need checking. Retry to check their status and continue.",
                                crate::tasks::removal::operations::Phase::ExternalComplete => "Linked issues were canceled. Retry to finish deleting the task.",
                                crate::tasks::removal::operations::Phase::LocalCommitted => "The task was deleted. Retry to finish the remaining updates.",
                                crate::tasks::removal::operations::Phase::Complete => "Deletion finished.",
                                crate::tasks::removal::operations::Phase::NeedsReconfirmation => "Task details changed. Stop this attempt before reviewing a new deletion.",
                                crate::tasks::removal::operations::Phase::Abandoned => "Deletion stopped. The task remains in Brain.",
                            },
                            serde_json::json!({"operation_id": operation.operation_id}),
                        ))
                    }
                }),
            };
            match outcome {
                Ok(screen) => state.screen = screen,
                Err(problem) => {
                    if let Some(id) = problem
                        .details
                        .get("operation_id")
                        .and_then(serde_json::Value::as_str)
                    {
                        state.operation_id = Some(id.into());
                    }
                    if problem.code == "stale_plan" {
                        state.notice =
                            "Task details changed. Review the updated preview before deleting."
                                .into();
                        if state.operation_id.is_some() {
                            state.notice.push_str(" An earlier deletion still needs attention. Retry it or stop that attempt before starting again.");
                        }
                        if visible {
                            self.removal_preview(state);
                            continue;
                        }
                    }
                    state.screen = RemovalScreen::Failed(problem);
                }
            }
            let mut dismiss = false;
            if let RemovalScreen::Finished(receipt) = &state.screen {
                dismiss = receipt.committed && receipt.warnings.is_empty();
                state.operation_id = None;
                if receipt.committed {
                    let selected = self.tasks.selected_identity().map(|(id, _)| id);
                    if let Err(error) = self.reload_tasks() {
                        dismiss = false;
                        state.notice =
                            format!("Task deleted, but refreshing the list failed: {error}");
                    }
                    self.tasks.restore_removal_selection(selected.as_deref());
                }
                self.status.set_flash(FlashKind::Info(
                    if receipt.committed {
                        "Task deleted"
                    } else {
                        "Task was not deleted"
                    }
                    .into(),
                ));
            } else if !visible && state.operation_id.is_some() {
                self.status.set_flash(FlashKind::Error(
                    "Task deletion needs attention; open Remove to continue".into(),
                ));
            }
            self.services.remember_removal(state.clone());
            if visible {
                if dismiss {
                    close_overlay(&mut self.overlay);
                } else {
                    replace_overlay(&mut self.overlay, Overlay::TaskRemoval(state));
                }
            }
        }
    }
}
