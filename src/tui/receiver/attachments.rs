//! Bounded background staging for durable receiver attachments.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, TryRecvError};

use crate::server::receiver::{InboundJob, StagedAttachment};
use crate::state::ReceiverJobId;
use crate::workspace::{CommandContext, WorkspaceContext};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReceiverAttachmentStage {
    job_id: ReceiverJobId,
    generation: u64,
}

impl ReceiverAttachmentStage {
    #[cfg(test)]
    #[must_use]
    pub(crate) const fn job_id(self) -> ReceiverJobId {
        self.job_id
    }
}

#[derive(Clone)]
pub(crate) struct ReceiverAttachmentRequest {
    stage: ReceiverAttachmentStage,
    command: CommandContext,
    message: InboundJob,
}

impl ReceiverAttachmentRequest {
    #[cfg(test)]
    #[must_use]
    pub(crate) const fn stage(&self) -> ReceiverAttachmentStage {
        self.stage
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) const fn message(&self) -> &InboundJob {
        &self.message
    }
}

enum ReceiverAttachmentWorkerOutcome {
    Ready(Vec<StagedAttachment>),
    Failed,
    Cancelled,
}

pub(crate) struct ReceiverAttachmentWorkerResult {
    stage: ReceiverAttachmentStage,
    outcome: ReceiverAttachmentWorkerOutcome,
}

impl ReceiverAttachmentWorkerResult {
    #[cfg(test)]
    #[must_use]
    pub(crate) fn success(stage: ReceiverAttachmentStage, staged: Vec<StagedAttachment>) -> Self {
        Self {
            stage,
            outcome: ReceiverAttachmentWorkerOutcome::Ready(staged),
        }
    }

    #[must_use]
    pub(crate) const fn failure(stage: ReceiverAttachmentStage) -> Self {
        Self {
            stage,
            outcome: ReceiverAttachmentWorkerOutcome::Failed,
        }
    }
}

pub(crate) trait ReceiverAttachmentRuntime: Send {
    fn start(&mut self, request: ReceiverAttachmentRequest) -> anyhow::Result<bool>;
    fn poll(&mut self) -> Option<ReceiverAttachmentWorkerResult>;
    fn cancel(&mut self);
    fn shutdown(&mut self);
}

pub(crate) enum ReceiverAttachmentEffect {
    Pending,
    Ready(PreparedReceiverAttachments),
    Failed,
}

pub(crate) struct PreparedReceiverAttachments {
    staged: Vec<StagedAttachment>,
}

impl PreparedReceiverAttachments {
    #[must_use]
    pub(crate) const fn empty() -> Self {
        Self { staged: Vec::new() }
    }

    #[must_use]
    pub(crate) fn staged(&self) -> &[StagedAttachment] {
        &self.staged
    }
}

impl Drop for PreparedReceiverAttachments {
    fn drop(&mut self) {
        cleanup_staged_attachments(&self.staged);
    }
}

pub(crate) struct ReceiverAttachmentCoordinator {
    runtime: Box<dyn ReceiverAttachmentRuntime>,
    active: Option<ReceiverAttachmentStage>,
    next_generation: u64,
}

impl ReceiverAttachmentCoordinator {
    #[must_use]
    pub(crate) fn system() -> Self {
        Self {
            runtime: Box::new(SystemReceiverAttachmentRuntime::new()),
            active: None,
            next_generation: 1,
        }
    }

    pub(crate) fn poll_or_start(
        &mut self,
        job_id: ReceiverJobId,
        command: &CommandContext,
        message: &InboundJob,
    ) -> ReceiverAttachmentEffect {
        if message.attachments.is_empty() {
            return ReceiverAttachmentEffect::Ready(PreparedReceiverAttachments::empty());
        }
        if message.attachments.len() > crate::server::receiver::MAX_ATTACHMENT_COUNT {
            return ReceiverAttachmentEffect::Failed;
        }
        if let Some(active) = self.active.filter(|active| active.job_id != job_id) {
            self.cancel(active.job_id);
        }
        if let Some(active) = self.active {
            let Some(result) = self.runtime.poll() else {
                return ReceiverAttachmentEffect::Pending;
            };
            if result.stage != active {
                cleanup_worker_outcome(result.outcome);
                return ReceiverAttachmentEffect::Pending;
            }
            self.active = None;
            return match result.outcome {
                ReceiverAttachmentWorkerOutcome::Ready(staged) => {
                    validate_staged_attachments(command.workspace.as_ref(), message, staged).map_or(
                        ReceiverAttachmentEffect::Failed,
                        ReceiverAttachmentEffect::Ready,
                    )
                }
                ReceiverAttachmentWorkerOutcome::Failed
                | ReceiverAttachmentWorkerOutcome::Cancelled => ReceiverAttachmentEffect::Failed,
            };
        }
        if let Some(stale) = self.runtime.poll() {
            cleanup_worker_outcome(stale.outcome);
            return ReceiverAttachmentEffect::Pending;
        }
        let stage = ReceiverAttachmentStage {
            job_id,
            generation: self.next_generation,
        };
        self.next_generation = self.next_generation.saturating_add(1);
        let request = ReceiverAttachmentRequest {
            stage,
            command: command.clone(),
            message: message.clone(),
        };
        match self.runtime.start(request) {
            Ok(true) => {
                self.active = Some(stage);
                ReceiverAttachmentEffect::Pending
            }
            Ok(false) => ReceiverAttachmentEffect::Pending,
            Err(_) => ReceiverAttachmentEffect::Failed,
        }
    }

    pub(crate) fn cancel(&mut self, job_id: ReceiverJobId) {
        if self.active.is_some_and(|active| active.job_id == job_id) {
            self.runtime.cancel();
            self.active = None;
            if let Some(result) = self.runtime.poll() {
                cleanup_worker_outcome(result.outcome);
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn replace(&mut self, runtime: Box<dyn ReceiverAttachmentRuntime>) {
        self.runtime.shutdown();
        self.runtime = runtime;
        self.active = None;
    }

    pub(crate) fn shutdown(&mut self) {
        self.runtime.shutdown();
        self.active = None;
    }
}

impl Drop for ReceiverAttachmentCoordinator {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct SystemAttachmentWorker {
    stage: ReceiverAttachmentStage,
    cancel: Arc<AtomicBool>,
    results: mpsc::Receiver<ReceiverAttachmentWorkerResult>,
    _thread: std::thread::JoinHandle<()>,
}

pub(crate) struct SystemReceiverAttachmentRuntime {
    worker: Option<SystemAttachmentWorker>,
}

impl SystemReceiverAttachmentRuntime {
    fn new() -> Self {
        Self { worker: None }
    }
}

impl ReceiverAttachmentRuntime for SystemReceiverAttachmentRuntime {
    fn start(&mut self, request: ReceiverAttachmentRequest) -> anyhow::Result<bool> {
        if self.worker.is_some() {
            return Ok(false);
        }
        let stage = request.stage;
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let (sender, results) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("brain-receiver-attachments".to_owned())
            .spawn(move || {
                let outcome = if worker_cancel.load(Ordering::Acquire) {
                    ReceiverAttachmentWorkerOutcome::Cancelled
                } else {
                    match crate::server::receiver::stage_attachments(
                        request.command.workspace.as_ref(),
                        &request.command,
                        &request.message,
                    ) {
                        Ok(staged) if worker_cancel.load(Ordering::Acquire) => {
                            cleanup_staged_attachments(&staged);
                            ReceiverAttachmentWorkerOutcome::Cancelled
                        }
                        Ok(staged) => ReceiverAttachmentWorkerOutcome::Ready(staged),
                        Err(_) => ReceiverAttachmentWorkerOutcome::Failed,
                    }
                };
                if let Err(error) = sender.send(ReceiverAttachmentWorkerResult { stage, outcome }) {
                    cleanup_worker_outcome(error.0.outcome);
                }
            })?;
        self.worker = Some(SystemAttachmentWorker {
            stage,
            cancel,
            results,
            _thread: worker,
        });
        Ok(true)
    }

    fn poll(&mut self) -> Option<ReceiverAttachmentWorkerResult> {
        let worker = self.worker.as_ref()?;
        match worker.results.try_recv() {
            Ok(result) => {
                self.worker = None;
                Some(result)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                let stage = worker.stage;
                self.worker = None;
                Some(ReceiverAttachmentWorkerResult::failure(stage))
            }
        }
    }

    fn cancel(&mut self) {
        if let Some(worker) = &self.worker {
            worker.cancel.store(true, Ordering::Release);
        }
    }

    fn shutdown(&mut self) {
        self.cancel();
        self.worker = None;
    }
}

impl Drop for SystemReceiverAttachmentRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn validate_staged_attachments(
    workspace: &WorkspaceContext,
    message: &InboundJob,
    mut staged: Vec<StagedAttachment>,
) -> anyhow::Result<PreparedReceiverAttachments> {
    let validation = (|| {
        anyhow::ensure!(
            message.attachments.len() <= crate::server::receiver::MAX_ATTACHMENT_COUNT,
            "receiver attachment count exceeds limit"
        );
        anyhow::ensure!(
            staged.len() == message.attachments.len(),
            "receiver attachment staging result count differs from accepted input"
        );
        let inbox = std::fs::canonicalize(workspace.paths().inbox_dir())
            .map_err(|_| anyhow::anyhow!("receiver attachment inbox is unavailable"))?;
        for attachment in &mut staged {
            anyhow::ensure!(
                attachment.error.is_none(),
                "receiver attachment staging failed"
            );
            let path = attachment
                .path
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("receiver attachment staging produced no file"))?;
            let canonical = std::fs::canonicalize(path)
                .map_err(|_| anyhow::anyhow!("receiver attachment file is unavailable"))?;
            anyhow::ensure!(
                canonical.starts_with(&inbox),
                "receiver attachment file is outside the workspace inbox"
            );
            let metadata = canonical
                .metadata()
                .map_err(|_| anyhow::anyhow!("receiver attachment metadata is unavailable"))?;
            anyhow::ensure!(
                metadata.is_file(),
                "receiver attachment result is not a file"
            );
            anyhow::ensure!(
                metadata.len() <= crate::server::receiver::MAX_ATTACHMENT_BYTES,
                "receiver attachment exceeds size limit"
            );
            attachment.path = Some(canonical);
        }
        Ok(())
    })();
    if let Err(error) = validation {
        cleanup_staged_attachments(&staged);
        return Err(error);
    }
    Ok(PreparedReceiverAttachments { staged })
}

fn cleanup_worker_outcome(outcome: ReceiverAttachmentWorkerOutcome) {
    if let ReceiverAttachmentWorkerOutcome::Ready(staged) = outcome {
        cleanup_staged_attachments(&staged);
    }
}

fn cleanup_staged_attachments(staged: &[StagedAttachment]) {
    for attachment in staged {
        if let Some(path) = &attachment.path {
            let _ = std::fs::remove_file(path);
        }
    }
}
