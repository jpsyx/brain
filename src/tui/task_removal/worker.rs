//! Native IO boundary. Detached workers own only a pinned command context.
use crate::{
    tasks::removal::{
        self,
        model::{Options, Problem, RemovalPlan},
        operations::{Operation, Receipt},
    },
    workspace::CommandContext,
};
use chrono::NaiveDate;
use std::{
    collections::VecDeque,
    sync::mpsc::{self, Receiver},
};

#[derive(Clone)]
pub(crate) enum Request {
    Preview {
        targets: Vec<String>,
        options: Options,
        today: NaiveDate,
    },
    Apply {
        plan: Box<RemovalPlan>,
        expected: String,
        today: NaiveDate,
    },
    Status(String),
    Resume(String),
    Abandon(String),
}
pub(crate) enum Response {
    Preview(Result<RemovalPlan, Problem>),
    Receipt(Result<Receipt, Problem>),
    Operation(Result<Box<Operation>, Problem>),
}
pub(crate) trait RemovalService {
    fn start(&mut self, context: CommandContext, generation: u64, request: Request);
    fn poll(&mut self) -> Option<(u64, Response)>;
}
#[derive(Default)]
pub(crate) struct NativeService {
    pending: Vec<(u64, Receiver<Response>)>,
    queued: VecDeque<(CommandContext, u64, Request)>,
}
impl RemovalService for NativeService {
    fn start(&mut self, context: CommandContext, generation: u64, request: Request) {
        if !self.pending.is_empty() {
            // Only a preview immediately superseded by another preview is disposable.
            // Preserve accepted durable and recovery requests in submission order.
            if matches!(request, Request::Preview { .. })
                && self
                    .queued
                    .back()
                    .is_some_and(|(_, _, previous)| matches!(previous, Request::Preview { .. }))
            {
                self.queued.pop_back();
            }
            self.queued.push_back((context, generation, request));
            return;
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        self.pending.push((generation, receiver));
        std::thread::spawn(move || {
            let result = match request {
                Request::Preview {
                    targets,
                    options,
                    today,
                } => Response::Preview(removal::preview(&context, &targets, options, today)),
                Request::Apply {
                    plan,
                    expected,
                    today,
                } => Response::Receipt(removal::apply(&context, &plan, &expected, today)),
                Request::Status(id) => {
                    Response::Operation(removal::operation_status(&context, &id).map(Box::new))
                }
                Request::Resume(id) => Response::Receipt(removal::resume(&context, &id)),
                Request::Abandon(id) => {
                    Response::Operation(removal::abandon(&context, &id).map(Box::new))
                }
            };
            let _ = sender.send(result);
        });
    }
    fn poll(&mut self) -> Option<(u64, Response)> {
        for index in 0..self.pending.len() {
            let response = match self.pending[index].1.try_recv() {
                Ok(value) => value,
                Err(mpsc::TryRecvError::Empty) => continue,
                Err(mpsc::TryRecvError::Disconnected) => Response::Receipt(Err(Problem::new(
                    "worker_failed",
                    "Removal worker stopped; inspect saved operation status before retrying",
                    serde_json::json!({}),
                ))),
            };
            let (generation, _) = self.pending.remove(index);
            if let Some((context, next_generation, request)) = self.queued.pop_front() {
                self.start(context, next_generation, request);
            }
            return Some((generation, response));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_task_removal_worker_bounds_inflight_previews_and_keeps_latest_generation() {
        use crate::workspace::{RegistryStore, WorkspaceContext, WorkspaceId, WorkspaceName};
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("workspace");
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::write(
            root.join("tasks/tasks.csv"),
            "task_id,task_name\nT1,Example\n",
        )
        .unwrap();
        let workspace = WorkspaceContext::new(
            temporary.path(),
            WorkspaceId::parse("e806258e-491a-436d-9db4-a5ca9903e0d4").unwrap(),
            WorkspaceName::parse("example").unwrap(),
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
        let plan = removal::preview(
            &context,
            &["T1".into()],
            Options::default(),
            chrono::Local::now().date_naive(),
        )
        .unwrap();
        let owner = crate::tasks::store_lock::TaskStoreOwner::acquire(&context.workspace).unwrap();
        let mut service = NativeService::default();
        for generation in 1..=3 {
            service.start(
                context.clone(),
                generation,
                Request::Preview {
                    targets: vec!["T1".into()],
                    options: Options::default(),
                    today: chrono::Local::now().date_naive(),
                },
            );
        }
        service.start(context.clone(), 4, Request::Status("absent".into()));
        service.start(context.clone(), 5, Request::Resume("absent".into()));
        service.start(context.clone(), 6, Request::Abandon("absent".into()));
        service.start(
            context.clone(),
            7,
            Request::Apply {
                expected: plan.fingerprint.clone(),
                plan: Box::new(plan),
                today: chrono::Local::now().date_naive(),
            },
        );
        for generation in 8..=9 {
            service.start(
                context.clone(),
                generation,
                Request::Preview {
                    targets: vec!["T1".into()],
                    options: Options::default(),
                    today: chrono::Local::now().date_naive(),
                },
            );
        }
        let in_flight = service.pending.len();
        drop(owner);
        assert_eq!(
            in_flight, 1,
            "preview replacement must not accumulate lock-waiting workers"
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut generations = Vec::new();
        while generations.len() < 7 {
            if let Some((generation, _)) = service.poll() {
                generations.push(generation);
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(generations, [1, 3, 4, 5, 6, 7, 9]);
    }
}
