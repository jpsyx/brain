//! Bounded, in-memory receiver admission and FIFO dispatch.

use std::collections::VecDeque;
use std::sync::Arc;

use crate::server::receiver::{ControlCommand, InboundJob, RestartPlan, parse_control_command};

const CAPACITY: usize = 64;

/// One socket admission that has appended but not yet committed its job.
///
/// Its fields stay private so only its issuing queue can decide which staged
/// append a failed acknowledgement is allowed to roll back.
#[derive(Debug)]
pub struct StagedAdmission {
    queue_identity: Arc<()>,
    generation: u64,
}

/// Why a job could not enter the live TUI's queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageError {
    /// The bounded queue already contains 64 jobs.
    Full,
    /// One socket transaction is already awaiting final acknowledgement.
    AdmissionInProgress,
}

impl std::fmt::Display for StageError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Full => formatter.write_str("inbound queue is full"),
            Self::AdmissionInProgress => {
                formatter.write_str("inbound queue admission is already in progress")
            }
        }
    }
}

impl std::error::Error for StageError {}

/// The live TUI's bounded FIFO of authenticated receiver work.
#[derive(Debug, Default)]
pub struct InboundQueue {
    // Allocation identity stays stable when the queue moves, and the token
    // keeps it alive until the admission is consumed.
    identity: Arc<()>,
    jobs: VecDeque<InboundJob>,
    staged: Option<u64>,
    next_admission: u64,
}

impl InboundQueue {
    /// Whether another socket transaction may begin staging a job.
    #[must_use]
    pub fn can_stage(&self) -> bool {
        self.jobs.len() < CAPACITY && self.staged.is_none()
    }

    /// Stage one validated job before the socket sends its final acknowledgement.
    ///
    /// # Errors
    ///
    /// Returns [`StageError::Full`] at the 64-job boundary, or
    /// [`StageError::AdmissionInProgress`] if a prior socket transaction has
    /// not yet finalized or rolled back.
    pub fn stage(&mut self, job: InboundJob) -> Result<StagedAdmission, StageError> {
        if self.jobs.len() >= CAPACITY {
            return Err(StageError::Full);
        }
        if self.staged.is_some() {
            return Err(StageError::AdmissionInProgress);
        }
        let admission = self.next_admission;
        self.next_admission = self.next_admission.wrapping_add(1);
        self.jobs.push_back(job);
        self.staged = Some(admission);
        Ok(StagedAdmission {
            queue_identity: Arc::clone(&self.identity),
            generation: admission,
        })
    }

    /// Make the exact staged admission visible to dispatch.
    #[must_use]
    #[allow(clippy::needless_pass_by_value)] // Consuming the token prevents admission replay.
    pub fn finalize(&mut self, admission: StagedAdmission) -> bool {
        if Arc::ptr_eq(&self.identity, &admission.queue_identity)
            && self.staged == Some(admission.generation)
        {
            self.staged = None;
            true
        } else {
            false
        }
    }

    /// Remove only the tail appended by this exact staged admission.
    #[allow(clippy::needless_pass_by_value)] // Consuming the token prevents rollback replay.
    pub fn rollback(&mut self, admission: StagedAdmission) -> Option<InboundJob> {
        (Arc::ptr_eq(&self.identity, &admission.queue_identity)
            && self.staged == Some(admission.generation))
        .then(|| {
            self.staged = None;
            self.jobs
                .pop_back()
                .expect("a staged admission always owns the queue tail")
        })
    }

    /// Oldest finalized job available for dispatch.
    #[must_use]
    pub fn head(&self) -> Option<&InboundJob> {
        if self.staged.is_some() && self.jobs.len() == 1 {
            None
        } else {
            self.jobs.front()
        }
    }

    /// Remove the FIFO head only after a successful agent launch.
    pub fn commit_head(&mut self, launch_succeeded: bool) -> Option<InboundJob> {
        (launch_succeeded && self.head().is_some()).then(|| {
            self.jobs
                .pop_front()
                .expect("a dispatchable head was just observed")
        })
    }

    /// Apply the first queued restart command as a cut through the FIFO.
    pub fn take_restart(&mut self) -> Option<RestartPlan<InboundJob>> {
        let finalized_len = self.jobs.len() - usize::from(self.staged.is_some());
        let at =
            self.jobs.iter().take(finalized_len).position(|job| {
                parse_control_command(&job.prompt) == Some(ControlCommand::Restart)
            })?;
        let dropped = self.jobs.drain(..at).collect();
        let command = self
            .jobs
            .pop_front()
            .expect("the located restart is now at the queue head");
        Some(RestartPlan { command, dropped })
    }

    /// Consume a new-session command only when it reaches the FIFO head.
    pub fn take_new_session(&mut self) -> Option<InboundJob> {
        (self.head().is_some_and(|job| {
            parse_control_command(&job.prompt) == Some(ControlCommand::NewSession)
        }))
        .then(|| {
            self.jobs
                .pop_front()
                .expect("the new-session command was just observed at the head")
        })
    }

    /// Number of jobs consuming bounded queue capacity, including a staged tail.
    #[must_use]
    pub fn len(&self) -> usize {
        self.jobs.len()
    }

    /// Whether no staged or finalized jobs consume queue capacity.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }

    /// Read-only owned view for integration and behavior tests.
    #[must_use]
    pub fn snapshot(&self) -> Vec<InboundJob> {
        self.jobs.iter().cloned().collect()
    }
}
