//! Captive choices shared by rendering and input; navigation never executes them.
use super::{RemovalScreen, RemovalState};
use crate::tasks::removal::{
    model::{Dependents, LinkedIssue},
    presentation,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Close,
    Delete,
    Dependencies(Dependents),
    Issue(LinkedIssue),
    Preview,
    Status,
    Resume,
    Abandon,
    ConfirmAbandon,
    Back,
}

pub(super) fn choices(state: &RemovalState) -> Vec<(Action, String)> {
    if state.abandon_confirm {
        return vec![
            (Action::Back, "Go back".into()),
            (Action::ConfirmAbandon, "Stop trying to delete".into()),
        ];
    }
    if state.recovery_available() {
        return vec![
            (Action::Close, "Close".into()),
            (Action::Status, "Check progress".into()),
            (Action::Resume, "Retry deletion".into()),
            (Action::Abandon, "Stop trying to delete...".into()),
        ];
    }
    let mut rows = vec![(Action::Close, "Cancel".into())];
    match &state.screen {
        RemovalScreen::Preview(plan) => {
            if plan.blockers.is_empty() {
                rows.push((Action::Delete, "Delete task".into()));
            }
            let affected = plan.options.dependents != Dependents::Reject
                || plan.blockers.iter().any(|p| p.code == "dependents_exist")
                || presentation::project(plan)
                    .tasks
                    .iter()
                    .any(|r| r.field == "dependencies");
            if affected {
                for (value, label) in [
                    (
                        Dependents::Rewire,
                        "Wait for the tasks this one was waiting for",
                    ),
                    (Dependents::Unlink, "Stop waiting for this task"),
                ] {
                    rows.push((
                        Action::Dependencies(value),
                        mark(label, plan.options.dependents == value),
                    ));
                }
            }
            if has_linked_issue(plan) {
                let keep = plan.options.linked_issue == LinkedIssue::Keep
                    || (plan.options.linked_issue == LinkedIssue::Configured
                        && plan.external.is_empty()
                        && plan.blockers.is_empty());
                rows.push((
                    Action::Issue(LinkedIssue::Keep),
                    mark("Leave linked issues unchanged", keep),
                ));
                rows.push((
                    Action::Issue(LinkedIssue::Cancel),
                    mark("Cancel linked issues too", !keep),
                ));
            }
        }
        RemovalScreen::Failed(_) => rows.push((Action::Preview, "Try again".into())),
        RemovalScreen::Running { .. } | RemovalScreen::Finished(_) => rows[0].1 = "Close".into(),
    }
    rows
}
fn mark(label: &str, selected: bool) -> String {
    format!("{} {label}", if selected { "●" } else { "○" })
}
fn has_linked_issue(plan: &crate::tasks::removal::model::RemovalPlan) -> bool {
    use crate::tasks::complete::{field, parse_csv_bytes};
    !plan.external.is_empty()
        || plan.options.linked_issue != LinkedIssue::Configured
        || plan.blockers.iter().any(|p| {
            matches!(
                p.code.as_str(),
                "adapter_unavailable" | "shared_external_link"
            )
        })
        || plan.edits.iter().any(|edit| {
            edit.before
                .as_deref()
                .and_then(|bytes| parse_csv_bytes(bytes).ok())
                .is_some_and(|csv| {
                    csv.rows.iter().any(|row| {
                        plan.targets
                            .iter()
                            .any(|target| target.task_id == field(row, "task_id"))
                            && !field(row, "linear_issue").trim().is_empty()
                    })
                })
        })
}

#[cfg(test)]
#[path = "actions_tests.rs"]
mod tests;
