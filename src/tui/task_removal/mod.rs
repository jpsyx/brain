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
use worker::{Request, Response};

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
                operation_id: uuid::Uuid::new_v4().to_string(),
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
        let targets = match &request {
            Request::Apply { plan, .. } => Some(plan.targets.clone()),
            _ => None,
        };
        let accepted = matches!(
            request,
            Request::Apply { .. } | Request::Resume(_) | Request::Abandon(_)
        );
        let state = self.services.start_removal(
            self.context.command().clone(),
            state,
            request,
            self.tasks.snapshot_revision(),
        );
        if let Some(targets) = targets {
            self.tasks.bind_workspace(self.context.workspace().id());
            self.tasks.project_task_change(
                state.generation,
                targets,
                crate::tui::task_mutation::projection::Change::Remove,
            );
        }
        if accepted {
            close_overlay(&mut self.overlay);
        } else {
            replace_overlay(&mut self.overlay, Overlay::TaskRemoval(state));
        }
    }

    pub(crate) fn tick_removal(&mut self) {
        while let Some((generation, job, response)) = self.services.poll_removal() {
            let response = match response {
                Response::Settled(response, rows) => {
                    match rows {
                        Ok(rows) => {
                            if !self.tasks.adopt_task_snapshot(job.revision, rows) {
                                self.services.refresh_task_snapshot(
                                    self.context.command().clone(),
                                    self.tasks.snapshot_revision(),
                                );
                            }
                        }
                        Err(error) => self.task_change_error(format!(
                            "Saved task result could not refresh: {error}"
                        )),
                    }
                    *response
                }
                response => response,
            };
            let state = job.state;
            if matches!(response, Response::Refreshed) {
                continue;
            }
            let response = if let Response::Warning(response, warning) = response {
                self.task_change_error(warning);
                *response
            } else {
                response
            };
            if let Response::Complete(result) = response {
                self.tasks.settle_task_change(generation, result.is_ok());
                match result {
                    Ok(result) => self.status.set_flash(FlashKind::Info(format!(
                        "✓ {} marked complete",
                        result.task_id
                    ))),
                    Err(error) => self.task_change_error(error),
                }
                continue;
            }
            if matches!(response, Response::Disconnected) {
                self.tasks.mark_task_change_uncertain(generation);
                self.task_change_error("Task worker stopped. Save outcome is unknown; refresh and inspect deletion status before retrying.".into());
                if let Some(mut state) = state {
                    state.screen =
                        RemovalScreen::Failed(crate::tasks::removal::model::Problem::new(
                            "worker_failed",
                            "Worker stopped; check saved deletion status before retrying",
                            serde_json::json!({}),
                        ));
                    self.services.remember_removal(state);
                }
                continue;
            }
            let Some(mut state) = state else {
                continue;
            };
            let visible = matches!(&self.overlay, Some(Overlay::TaskRemoval(modal)) if modal.generation == generation);
            let outcome = match response {
                Response::Refreshed | Response::Complete(_) | Response::Settled(_, _) | Response::Warning(_, _) | Response::Disconnected => unreachable!(),
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
                    if job.preallocated_apply && problem.details.get("operation_id").is_none() {
                        state.operation_id = None;
                    }
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
                            self.tasks.settle_task_change(generation, false);
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
                    self.tasks.project_task_change(
                        generation,
                        receipt.removed.clone(),
                        crate::tui::task_mutation::projection::Change::Remove,
                    );
                    self.tasks.settle_task_change(generation, true);
                }
                self.status.set_flash(FlashKind::Info(
                    if receipt.committed {
                        "Task deleted"
                    } else {
                        "Task was not deleted"
                    }
                    .into(),
                ));
            } else if let RemovalScreen::Failed(problem) = &state.screen {
                self.tasks.settle_task_change(generation, false);
                self.task_change_error(format!(
                    "{}: {}. Open Remove to review or recover.",
                    state.targets.join(", "),
                    problem.message
                ));
            }
            if let RemovalScreen::Finished(receipt) = &state.screen {
                if !receipt.committed {
                    self.tasks.settle_task_change(generation, false);
                }
                if !receipt.warnings.is_empty() {
                    self.task_change_error(format!(
                        "Task deleted with warnings: {}",
                        receipt
                            .warnings
                            .iter()
                            .map(|p| p.message.as_str())
                            .collect::<Vec<_>>()
                            .join("; ")
                    ));
                }
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
