//! Native IO boundary. Detached workers own only a pinned command context.
use crate::{
    tasks::removal::{
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
    Refresh,
    Preview {
        targets: Vec<String>,
        options: Options,
        today: NaiveDate,
    },
    Apply {
        operation_id: String,
        plan: Box<RemovalPlan>,
        expected: String,
        today: NaiveDate,
    },
    Complete {
        identity: crate::tasks::removal::model::Identity,
        today: NaiveDate,
    },
    Status(String),
    Resume(String),
    Abandon(String),
}
pub(crate) enum Response {
    Refreshed,
    Complete(Result<crate::tasks::complete::CompletionResult, String>),
    Settled(Box<Self>, Result<super::Rows, String>),
    Disconnected,
    Warning(Box<Self>, String),
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
    active: Option<(CommandContext, Request, bool)>,
    handles: Vec<std::thread::JoinHandle<()>>,
    queued: VecDeque<(CommandContext, u64, Request)>,
}
impl RemovalService for NativeService {
    fn start(&mut self, context: CommandContext, generation: u64, request: Request) {
        if !self.pending.is_empty() {
            self.queued.push_back((context, generation, request));
            return;
        }
        self.launch(context, generation, request, false);
    }
    fn poll(&mut self) -> Option<(u64, Response)> {
        for index in (0..self.handles.len()).rev() {
            if self.handles[index].is_finished() {
                let _ = self.handles.swap_remove(index).join();
            }
        }
        for index in 0..self.pending.len() {
            let response = match self.pending[index].1.try_recv() {
                Ok(value) => value,
                Err(mpsc::TryRecvError::Empty) => continue,
                Err(mpsc::TryRecvError::Disconnected) => Response::Disconnected,
            };
            let (generation, _) = self.pending.remove(index);
            if let Some((context, request, false)) = self.active.take() {
                if matches!(response, Response::Disconnected) {
                    self.launch(context, generation, request, true);
                    return None;
                }
            }
            if let Some((context, next_generation, request)) = self.queued.pop_front() {
                self.start(context, next_generation, request);
            }
            return Some((generation, response));
        }
        None
    }
}

impl NativeService {
    fn launch(
        &mut self,
        context: CommandContext,
        generation: u64,
        request: Request,
        reconcile: bool,
    ) {
        let (sender, receiver) = mpsc::sync_channel(1);
        self.active = Some((context.clone(), request.clone(), reconcile));
        self.pending.push((generation, receiver));
        self.handles.push(std::thread::spawn(move || {
            let response = if reconcile {
                super::native::reconcile(&context, &request)
            } else {
                super::native::execute(&context, request)
            };
            let _ = sender.send(response);
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_task_removal_worker_bounds_inflight_work_and_preserves_fifo_requests() {
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
        let plan = crate::tasks::removal::preview(
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
                operation_id: uuid::Uuid::new_v4().to_string(),
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
        while generations.len() < 9 {
            if let Some((generation, _)) = service.poll() {
                generations.push(generation);
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(generations, [1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }
}
