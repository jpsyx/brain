use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::tui::receiver::attachments::{
    ReceiverAttachmentRequest, ReceiverAttachmentRuntime, ReceiverAttachmentStage,
    ReceiverAttachmentWorkerResult,
};

use super::receiver_durable_support::ReceiverClock;

#[derive(Clone, Default)]
pub(super) struct ControlledAttachmentWorker {
    state: Arc<Mutex<ControlledAttachmentState>>,
}

#[derive(Default)]
struct ControlledAttachmentState {
    starts: Vec<ReceiverAttachmentRequest>,
    completions: VecDeque<ReceiverAttachmentWorkerResult>,
    cancellations: usize,
    shutdowns: usize,
    advance_on_poll: Option<(ReceiverClock, std::time::Duration)>,
    advance_on_shutdown: Option<(ReceiverClock, std::time::Duration)>,
}

impl ControlledAttachmentWorker {
    pub(super) fn starts(&self) -> usize {
        self.state.lock().expect("attachment worker").starts.len()
    }

    pub(super) fn cancellations(&self) -> usize {
        self.state.lock().expect("attachment worker").cancellations
    }

    pub(super) fn shutdowns(&self) -> usize {
        self.state.lock().expect("attachment worker").shutdowns
    }

    pub(super) fn stage(&self, index: usize) -> ReceiverAttachmentStage {
        self.state.lock().expect("attachment worker").starts[index].stage()
    }

    pub(super) fn complete(
        &self,
        stage: ReceiverAttachmentStage,
        staged: Vec<crate::server::receiver::StagedAttachment>,
    ) {
        self.state
            .lock()
            .expect("attachment worker")
            .completions
            .push_back(ReceiverAttachmentWorkerResult::success(stage, staged));
    }

    pub(super) fn complete_with_cleanup_clock(
        &self,
        stage: ReceiverAttachmentStage,
        directory: std::path::PathBuf,
        staged: Vec<crate::server::receiver::StagedAttachment>,
        clock: ReceiverClock,
        duration: std::time::Duration,
    ) {
        self.state
            .lock()
            .expect("attachment worker")
            .completions
            .push_back(
                ReceiverAttachmentWorkerResult::success_with_owned_cleanup_observer(
                    stage,
                    directory,
                    staged,
                    Box::new(move || clock.advance(duration)),
                ),
            );
    }

    pub(super) fn fail(&self, stage: ReceiverAttachmentStage) {
        self.state
            .lock()
            .expect("attachment worker")
            .completions
            .push_back(ReceiverAttachmentWorkerResult::failure(stage));
    }

    pub(super) fn advance_on_next_poll(&self, clock: ReceiverClock, duration: std::time::Duration) {
        self.state
            .lock()
            .expect("attachment worker")
            .advance_on_poll = Some((clock, duration));
    }

    pub(super) fn advance_on_shutdown(&self, clock: ReceiverClock, duration: std::time::Duration) {
        self.state
            .lock()
            .expect("attachment worker")
            .advance_on_shutdown = Some((clock, duration));
    }
}

impl ReceiverAttachmentRuntime for ControlledAttachmentWorker {
    fn start(&mut self, request: ReceiverAttachmentRequest) -> anyhow::Result<bool> {
        self.state
            .lock()
            .expect("attachment worker")
            .starts
            .push(request);
        Ok(true)
    }

    fn poll(&mut self) -> Option<ReceiverAttachmentWorkerResult> {
        let advance = self
            .state
            .lock()
            .expect("attachment worker")
            .advance_on_poll
            .take();
        if let Some((clock, duration)) = advance {
            clock.advance(duration);
        }
        self.state
            .lock()
            .expect("attachment worker")
            .completions
            .pop_front()
    }

    fn cancel(&mut self) {
        self.state.lock().expect("attachment worker").cancellations += 1;
    }

    fn shutdown(&mut self) {
        let advance = {
            let mut state = self.state.lock().expect("attachment worker");
            state.shutdowns += 1;
            state.advance_on_shutdown.take()
        };
        if let Some((clock, duration)) = advance {
            clock.advance(duration);
        }
    }
}
