//! Bounded background staging for durable receiver attachments.

use std::sync::mpsc::{self, TryRecvError};

use crate::server::receiver::{InboundJob, StagedAttachment, StagedAttachmentBatch};
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
    Ready(StagedAttachmentBatch),
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
            outcome: ReceiverAttachmentWorkerOutcome::Ready(StagedAttachmentBatch::unowned(staged)),
        }
    }

    #[cfg(test)]
    pub(crate) fn success_with_owned_cleanup_observer(
        stage: ReceiverAttachmentStage,
        directory: std::path::PathBuf,
        staged: Vec<StagedAttachment>,
        after_cleanup: Box<dyn FnOnce() + Send>,
    ) -> Self {
        Self {
            stage,
            outcome: ReceiverAttachmentWorkerOutcome::Ready(
                StagedAttachmentBatch::new(directory, staged).observe_cleanup(after_cleanup),
            ),
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
    batch: StagedAttachmentBatch,
}

impl PreparedReceiverAttachments {
    #[must_use]
    pub(crate) const fn empty() -> Self {
        Self {
            batch: StagedAttachmentBatch::empty(),
        }
    }

    #[must_use]
    pub(crate) fn staged(&self) -> &[StagedAttachment] {
        self.batch.staged()
    }
}

pub(crate) struct ReceiverAttachmentCoordinator {
    runtime: Box<dyn ReceiverAttachmentRuntime>,
    active: Option<ReceiverAttachmentStage>,
    next_generation: u64,
    stopped: bool,
}

impl ReceiverAttachmentCoordinator {
    #[must_use]
    pub(crate) fn system() -> Self {
        Self {
            runtime: Box::new(SystemReceiverAttachmentRuntime::new()),
            active: None,
            next_generation: 1,
            stopped: false,
        }
    }

    pub(crate) fn poll_or_start(
        &mut self,
        job_id: ReceiverJobId,
        command: &CommandContext,
        message: &InboundJob,
    ) -> ReceiverAttachmentEffect {
        if self.stopped {
            return ReceiverAttachmentEffect::Failed;
        }
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
            drop(stale);
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
            drop(self.runtime.poll());
        }
    }

    #[cfg(test)]
    pub(crate) fn replace(&mut self, runtime: Box<dyn ReceiverAttachmentRuntime>) {
        self.runtime.shutdown();
        self.runtime = runtime;
        self.active = None;
        self.stopped = false;
    }

    pub(crate) fn shutdown(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
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
    cancel: crate::server::provider::CurlCancellation,
    results: mpsc::Receiver<ReceiverAttachmentWorkerResult>,
    thread: std::thread::JoinHandle<()>,
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
        let cancel = crate::server::provider::CurlCancellation::new();
        let worker_cancel = cancel.clone();
        let (sender, results) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("brain-receiver-attachments".to_owned())
            .spawn(move || {
                let outcome = if worker_cancel.is_cancelled() {
                    ReceiverAttachmentWorkerOutcome::Cancelled
                } else {
                    match crate::server::receiver::stage_attachments_cancellable(
                        request.command.workspace.as_ref(),
                        &request.command,
                        &request.message,
                        &worker_cancel,
                    ) {
                        Ok(staged) if worker_cancel.is_cancelled() => {
                            drop(staged);
                            ReceiverAttachmentWorkerOutcome::Cancelled
                        }
                        Ok(staged) => ReceiverAttachmentWorkerOutcome::Ready(staged),
                        Err(_) => ReceiverAttachmentWorkerOutcome::Failed,
                    }
                };
                let _ = sender.send(ReceiverAttachmentWorkerResult { stage, outcome });
            })?;
        self.worker = Some(SystemAttachmentWorker {
            stage,
            cancel,
            results,
            thread: worker,
        });
        Ok(true)
    }

    fn poll(&mut self) -> Option<ReceiverAttachmentWorkerResult> {
        let received = self.worker.as_ref()?.results.try_recv();
        match received {
            Ok(result) => {
                let worker = self.worker.take().expect("attachment worker exists");
                let _ = worker.thread.join();
                Some(result)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                let worker = self.worker.take().expect("attachment worker exists");
                let stage = worker.stage;
                let _ = worker.thread.join();
                Some(ReceiverAttachmentWorkerResult::failure(stage))
            }
        }
    }

    fn cancel(&mut self) {
        if let Some(worker) = &self.worker {
            worker.cancel.cancel();
        }
    }

    fn shutdown(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.cancel.cancel();
            let _ = worker.thread.join();
        }
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
    mut batch: StagedAttachmentBatch,
) -> anyhow::Result<PreparedReceiverAttachments> {
    let validation = (|| {
        anyhow::ensure!(
            message.attachments.len() <= crate::server::receiver::MAX_ATTACHMENT_COUNT,
            "receiver attachment count exceeds limit"
        );
        anyhow::ensure!(
            batch.staged().len() == message.attachments.len(),
            "receiver attachment staging result count differs from accepted input"
        );
        let inbox = std::fs::canonicalize(workspace.paths().inbox_dir())
            .map_err(|_| anyhow::anyhow!("receiver attachment inbox is unavailable"))?;
        for attachment in batch.staged_mut() {
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
    validation?;
    Ok(PreparedReceiverAttachments { batch })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropping_an_unread_ready_result_removes_its_exact_batch_directory() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let job_dir = temporary.path().join("unread-job");
        std::fs::create_dir(&job_dir).expect("job directory");
        let staged_path = job_dir.join("private.txt");
        let partial_path = job_dir.join("other.part");
        std::fs::write(&staged_path, b"private staged media").expect("staged media");
        std::fs::write(&partial_path, b"private partial media").expect("partial media");
        let stage = ReceiverAttachmentStage {
            job_id: uuid::Uuid::new_v4().into(),
            generation: 1,
        };
        let result = ReceiverAttachmentWorkerResult {
            stage,
            outcome: ReceiverAttachmentWorkerOutcome::Ready(StagedAttachmentBatch::new(
                job_dir.clone(),
                vec![StagedAttachment {
                    source: "provider-id".to_owned(),
                    path: Some(staged_path),
                    error: None,
                }],
            )),
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        sender.send(result).expect("queue worker result");

        drop(receiver);

        assert!(!job_dir.exists());
    }

    #[test]
    fn system_runtime_shutdown_kills_and_reaps_the_published_provider_group() {
        let cancellation = crate::server::provider::CurlCancellation::new();
        let worker_cancellation = cancellation.clone();
        let (published_sender, published_receiver) = mpsc::sync_channel(1);
        let (result_sender, results) = mpsc::sync_channel(1);
        let stage = ReceiverAttachmentStage {
            job_id: uuid::Uuid::new_v4().into(),
            generation: 1,
        };
        let thread = std::thread::spawn(move || {
            let mut command = std::process::Command::new("/bin/sh");
            command
                .args(["-c", "read _"])
                .stdin(std::process::Stdio::piped());
            let _ = worker_cancellation.run_for_test(command, |pid| {
                published_sender.send(pid).expect("publish provider PID");
            });
            let _ = result_sender.send(ReceiverAttachmentWorkerResult {
                stage,
                outcome: ReceiverAttachmentWorkerOutcome::Cancelled,
            });
        });
        let published = published_receiver.recv().expect("published provider PID");
        let mut runtime = SystemReceiverAttachmentRuntime {
            worker: Some(SystemAttachmentWorker {
                stage,
                cancel: cancellation,
                results,
                thread,
            }),
        };

        ReceiverAttachmentRuntime::shutdown(&mut runtime);

        assert!(runtime.worker.is_none());
        let pid = i32::try_from(published).expect("PID range");
        assert_eq!(
            nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None),
            Err(nix::errno::Errno::ESRCH)
        );
    }
}
