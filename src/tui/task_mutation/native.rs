//! Worker-owned IO; no App or event-loop services cross this boundary.
use super::worker::{Request, Response};
use crate::{
    tasks::{removal, store_lock::Acquisition},
    workspace::CommandContext,
};

pub(super) fn execute(context: &CommandContext, request: Request) -> Response {
    if let Request::Preview {
        targets,
        options,
        today,
    } = request
    {
        return Response::Preview(removal::preview_with(
            context,
            &targets,
            &options,
            today,
            Acquisition::Queued,
        ));
    }
    let recover = !matches!(request, Request::Status(_));
    let response = match request {
        Request::Complete { identity, today } => {
            let result = crate::tasks::complete::queued::run(context, &identity, today);
            let mut warning = None;
            if let Ok(result) = &result {
                let targets = crate::tasks::agenda::resolve_targets(
                    &context.registry_store,
                    &context.workspace,
                    today,
                );
                let existed = targets.markdown.exists();
                let pdf_existed = targets.pdf.exists();
                let outcome = crate::tasks::agenda::sync_targets(
                    &targets,
                    &result.task_id,
                    crate::tasks::agenda::Action::Done,
                    today,
                );
                if (existed && outcome == crate::tasks::agenda::Outcome::NoAgenda)
                    || (pdf_existed
                        && outcome == crate::tasks::agenda::Outcome::Updated { pdf: false })
                {
                    warning = Some(format!(
                        "{} saved, but agenda refresh failed",
                        result.task_id
                    ));
                }
            }
            let response = Response::Complete(result.map_err(|error| {
                format!("{} ({}): {error}", identity.task_id, identity.task_name)
            }));
            if let Some(warning) = warning {
                Response::Warning(Box::new(response), warning)
            } else {
                response
            }
        }
        Request::Apply {
            plan,
            expected,
            today,
            operation_id,
        } => Response::Receipt(removal::apply_queued(
            context,
            &plan,
            &expected,
            today,
            operation_id,
            Acquisition::Queued,
            |_| Ok(()),
        )),
        Request::Status(id) => {
            Response::Operation(removal::operation_status(context, &id).map(Box::new))
        }
        Request::Resume(id) => {
            Response::Receipt(removal::resume_with(context, &id, Acquisition::Queued))
        }
        Request::Abandon(id) => Response::Operation(
            removal::abandon_with(context, &id, Acquisition::Queued).map(Box::new),
        ),
        Request::Preview { .. } => unreachable!(),
    };
    let rows = load_rows(context, recover);
    let response = if let Response::Receipt(Err(problem)) = response {
        let receipt = problem
            .details
            .get("operation_id")
            .and_then(serde_json::Value::as_str)
            .and_then(|id| removal::operation_status(context, id).ok())
            .and_then(|operation| operation.receipt);
        Response::Receipt(receipt.ok_or(problem))
    } else {
        response
    };
    Response::Settled(Box::new(response), rows)
}

fn load_rows(context: &CommandContext, recover: bool) -> Result<super::Rows, String> {
    let load = || -> anyhow::Result<super::Rows> {
        let _owner = if recover {
            Acquisition::Queued.acquire(&context.workspace)?
        } else {
            Acquisition::Queued.inspect(&context.workspace)?
        };
        let directory = context.workspace.root().join("tasks");
        let tasks = crate::tasks::task::load_tasks(&directory.join("tasks.csv"))?;
        let habits_path = directory.join("habits.csv");
        let habits = if habits_path.exists() {
            crate::tasks::task::load_habits(&habits_path)?
        } else {
            Vec::new()
        };
        Ok((tasks, habits))
    };
    load().map_err(|error| error.to_string())
}

/// Reconcile evidence only after a disconnected worker. Never replay its mutation.
pub(super) fn reconcile(context: &CommandContext, request: &Request) -> Response {
    let rows = load_rows(
        context,
        !matches!(request, Request::Preview { .. } | Request::Status(_)),
    );
    let response = match request {
        Request::Apply { operation_id, .. }
        | Request::Resume(operation_id)
        | Request::Status(operation_id)
        | Request::Abandon(operation_id) => reconcile_removal(context, operation_id),
        Request::Complete { identity, .. } => {
            let done = rows
                .as_ref()
                .ok()
                .and_then(|(tasks, habits)| {
                    let rows = if identity.csv == crate::tasks::identity::CsvKind::Tasks {
                        tasks
                    } else {
                        habits
                    };
                    rows.iter().find(|task| {
                        task.task_uuid.unwrap_or_else(|| {
                            crate::tasks::identity::legacy_task_uuid(
                                context.workspace.id(),
                                identity.csv,
                                &task.id,
                            )
                        }) == identity.task_uuid
                    })
                })
                .filter(|task| task.is_done());
            done.map_or_else(
                || {
                    if rows.is_ok() {
                        Response::Complete(Err(format!(
                            "{} ({}): worker stopped before completion was saved",
                            identity.task_id, identity.task_name
                        )))
                    } else {
                        Response::Disconnected
                    }
                },
                |task| {
                    Response::Complete(Ok(crate::tasks::complete::CompletionResult {
                        kind: if task.is_habit() {
                            crate::tasks::complete::CompletionKind::Habit
                        } else {
                            crate::tasks::complete::CompletionKind::Task
                        },
                        task_id: task.id.clone(),
                        task_name: task.name.clone(),
                        next_id: None,
                        next_due: None,
                        mit_migrated_to: None,
                        project: None,
                        linear_issue: None,
                    }))
                },
            )
        }
        Request::Preview { .. } => Response::Preview(Err(removal::model::Problem::new(
            "worker_failed",
            "Could not check deletion details. Nothing was deleted by this preview.",
            serde_json::json!({}),
        ))),
    };
    Response::Settled(
        Box::new(Response::Warning(
            Box::new(response),
            "Task worker stopped; save evidence was checked without replaying the request.".into(),
        )),
        rows,
    )
}

fn reconcile_removal(context: &CommandContext, id: &str) -> Response {
    match removal::interrupted(context, id) {
        Err(problem) if problem.code == "outcome_unknown" => Response::Disconnected,
        result => Response::Receipt(result),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disconnected_completion_reconciles_not_done_without_replaying() {
        use crate::workspace::{RegistryStore, WorkspaceContext, WorkspaceId, WorkspaceName};
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("workspace");
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::write(root.join("tasks/tasks.csv"), "task_id,task_name\n").unwrap();
        let workspace = WorkspaceContext::new(
            temporary.path(),
            WorkspaceId::new(),
            WorkspaceName::parse("test").unwrap(),
            &root,
            "member",
            temporary.path(),
        )
        .unwrap();
        let context = CommandContext::for_test(
            std::sync::Arc::new(workspace),
            RegistryStore::from_path(temporary.path().join("env.json")),
            "member",
        );
        let identity = removal::model::Identity {
            csv: crate::tasks::identity::CsvKind::Tasks,
            task_uuid: crate::tasks::identity::TaskUuid::new(),
            task_id: "T1".into(),
            task_name: "Missing".into(),
        };
        let response = reconcile(
            &context,
            &Request::Complete {
                identity,
                today: chrono::Local::now().date_naive(),
            },
        );
        assert!(
            matches!(response,Response::Settled(response,Ok(_)) if matches!(&*response,Response::Warning(response, _) if matches!(**response,Response::Complete(Err(_)))))
        );
        assert_eq!(
            std::fs::read_to_string(root.join("tasks/tasks.csv")).unwrap(),
            "task_id,task_name\n"
        );
    }
}
