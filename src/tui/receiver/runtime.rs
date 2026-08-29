//! Bounded receiver effects; the state DB remains the scheduling authority.

use std::time::Instant;

use super::{DurableReceiverRun, ReceiverAnswerControllerCleanup};

const MAX_ANSWER_CONTROLLER_CLEANUPS: usize = 8;

mod sync;

pub(crate) use sync::{SyncGateObservation, SyncGatePoll};

#[cfg(test)]
type BeforeObservationPersistenceHook = Box<dyn FnOnce(&[crate::agent::AgentObservationBoundary])>;

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReceiverLaunchBoundary {
    CapabilityPlanning,
    AvailabilityProbe,
    ResumeValidation,
    Registration,
    RecoveryPreLaunchAuthorization,
    RecoveryLaunchPreparation,
    Spawn,
    RecoveryLaunchCommit,
    Allocation,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReceiverCleanupBoundary {
    Shutdown,
    Session,
    Artifacts,
    Acknowledgement,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReceiverAnswerCleanupEvent {
    ControllerShutdown,
    SessionRelease,
    ArtifactCleanup,
    TaskReload,
    SyncLaunch,
}

struct ReceiverSyncGate {
    seen_journal_id: Option<i64>,
    launched_at: Instant,
    next_poll: Instant,
    attempts: u8,
}

pub(crate) struct ReceiverRuntime {
    enabled: bool,
    sync_gate: Option<ReceiverSyncGate>,
    // This effect state is revalidated against durable ownership on later ticks.
    durable_run: DurableReceiverRun,
    // Cleanup is bounded separately so completed answers cannot block the next claim.
    answer_controller_cleanups: std::collections::VecDeque<ReceiverAnswerControllerCleanup>,
    #[cfg(test)]
    after_restart_scan_hook: Option<Box<dyn FnOnce()>>,
    #[cfg(test)]
    after_completion_validation_hook: Option<Box<dyn FnOnce()>>,
    #[cfg(test)]
    after_completion_commit_hook: Option<Box<dyn FnOnce()>>,
    #[cfg(test)]
    after_observation_validation_hook: Option<Box<dyn FnOnce()>>,
    #[cfg(test)]
    before_observation_persistence_hook: Option<BeforeObservationPersistenceHook>,
    #[cfg(test)]
    launch_boundary_hooks: Vec<(ReceiverLaunchBoundary, Box<dyn FnOnce()>)>,
    #[cfg(test)]
    cleanup_failure_boundaries: Vec<ReceiverCleanupBoundary>,
    #[cfg(test)]
    answer_cleanup_failures: Vec<(crate::state::ReceiverJobId, ReceiverCleanupBoundary)>,
    #[cfg(test)]
    answer_cleanup_events: Vec<ReceiverAnswerCleanupEvent>,
    #[cfg(test)]
    recovery_tab_error: Option<crate::tui::state::ReceiverRunTabError>,
    #[cfg(test)]
    observation_diagnostics: std::cell::RefCell<Vec<String>>,
}

impl ReceiverRuntime {
    #[must_use]
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            sync_gate: None,
            durable_run: DurableReceiverRun::Idle,
            answer_controller_cleanups: std::collections::VecDeque::new(),
            #[cfg(test)]
            after_restart_scan_hook: None,
            #[cfg(test)]
            after_completion_validation_hook: None,
            #[cfg(test)]
            after_completion_commit_hook: None,
            #[cfg(test)]
            after_observation_validation_hook: None,
            #[cfg(test)]
            before_observation_persistence_hook: None,
            #[cfg(test)]
            launch_boundary_hooks: Vec::new(),
            #[cfg(test)]
            cleanup_failure_boundaries: Vec::new(),
            #[cfg(test)]
            answer_cleanup_failures: Vec::new(),
            #[cfg(test)]
            answer_cleanup_events: Vec::new(),
            #[cfg(test)]
            recovery_tab_error: None,
            #[cfg(test)]
            observation_diagnostics: std::cell::RefCell::new(Vec::new()),
        }
    }

    #[cfg(test)]
    pub(crate) fn install_after_restart_scan_hook(&mut self, hook: Box<dyn FnOnce()>) {
        self.after_restart_scan_hook = Some(hook);
    }

    #[cfg(test)]
    pub(crate) fn run_after_restart_scan_hook(&mut self) {
        if let Some(hook) = self.after_restart_scan_hook.take() {
            hook();
        }
    }

    #[cfg(test)]
    pub(crate) fn install_after_completion_validation_hook(&mut self, hook: Box<dyn FnOnce()>) {
        self.after_completion_validation_hook = Some(hook);
    }

    #[cfg(test)]
    pub(crate) fn run_after_completion_validation_hook(&mut self) {
        if let Some(hook) = self.after_completion_validation_hook.take() {
            hook();
        }
    }

    #[cfg(test)]
    pub(crate) fn install_after_completion_commit_hook(&mut self, hook: Box<dyn FnOnce()>) {
        self.after_completion_commit_hook = Some(hook);
    }

    #[cfg(test)]
    pub(crate) fn run_after_completion_commit_hook(&mut self) {
        if let Some(hook) = self.after_completion_commit_hook.take() {
            hook();
        }
    }

    #[cfg(test)]
    pub(crate) fn install_after_observation_validation_hook(&mut self, hook: Box<dyn FnOnce()>) {
        self.after_observation_validation_hook = Some(hook);
    }

    #[cfg(test)]
    pub(crate) fn run_after_observation_validation_hook(&mut self) {
        if let Some(hook) = self.after_observation_validation_hook.take() {
            hook();
        }
    }

    #[cfg(test)]
    pub(crate) fn install_before_observation_persistence_hook(
        &mut self,
        hook: BeforeObservationPersistenceHook,
    ) {
        self.before_observation_persistence_hook = Some(hook);
    }

    #[cfg(test)]
    pub(crate) fn run_before_observation_persistence_hook(
        &mut self,
        boundaries: &[crate::agent::AgentObservationBoundary],
    ) {
        if let Some(hook) = self.before_observation_persistence_hook.take() {
            hook(boundaries);
        }
    }

    #[cfg(test)]
    pub(crate) fn install_launch_boundary_hook(
        &mut self,
        boundary: ReceiverLaunchBoundary,
        hook: Box<dyn FnOnce()>,
    ) {
        self.launch_boundary_hooks.push((boundary, hook));
    }

    #[cfg(test)]
    pub(crate) fn run_launch_boundary_hook(&mut self, boundary: ReceiverLaunchBoundary) {
        let Some(index) = self
            .launch_boundary_hooks
            .iter()
            .position(|(candidate, _)| *candidate == boundary)
        else {
            return;
        };
        let (_, hook) = self.launch_boundary_hooks.remove(index);
        hook();
    }

    #[cfg(test)]
    pub(crate) fn inject_cleanup_failure(&mut self, boundary: ReceiverCleanupBoundary) {
        self.cleanup_failure_boundaries.push(boundary);
    }

    #[cfg(test)]
    pub(crate) fn take_cleanup_failure(&mut self, boundary: ReceiverCleanupBoundary) -> bool {
        let Some(index) = self
            .cleanup_failure_boundaries
            .iter()
            .position(|candidate| *candidate == boundary)
        else {
            return false;
        };
        self.cleanup_failure_boundaries.remove(index);
        true
    }

    #[cfg(test)]
    pub(crate) fn inject_answer_cleanup_failure(
        &mut self,
        job_id: crate::state::ReceiverJobId,
        boundary: ReceiverCleanupBoundary,
    ) {
        self.answer_cleanup_failures.push((job_id, boundary));
    }

    #[cfg(test)]
    pub(crate) fn take_answer_cleanup_failure(
        &mut self,
        job_id: crate::state::ReceiverJobId,
        boundary: ReceiverCleanupBoundary,
    ) -> bool {
        let Some(index) = self
            .answer_cleanup_failures
            .iter()
            .position(|candidate| *candidate == (job_id, boundary))
        else {
            return false;
        };
        self.answer_cleanup_failures.remove(index);
        true
    }

    #[cfg(test)]
    pub(crate) fn record_answer_cleanup_event(&mut self, event: ReceiverAnswerCleanupEvent) {
        self.answer_cleanup_events.push(event);
    }

    #[cfg(test)]
    pub(crate) fn answer_cleanup_events(&self) -> &[ReceiverAnswerCleanupEvent] {
        &self.answer_cleanup_events
    }

    #[cfg(test)]
    pub(crate) fn inject_recovery_tab_error(
        &mut self,
        error: crate::tui::state::ReceiverRunTabError,
    ) {
        self.recovery_tab_error = Some(error);
    }

    #[cfg(test)]
    pub(crate) fn take_recovery_tab_error(
        &mut self,
    ) -> Option<crate::tui::state::ReceiverRunTabError> {
        self.recovery_tab_error.take()
    }

    #[cfg(test)]
    pub(crate) fn record_observation_diagnostic(&self, diagnostic: String) {
        self.observation_diagnostics.borrow_mut().push(diagnostic);
    }

    #[cfg(test)]
    pub(crate) fn last_observation_diagnostic(&self) -> Option<String> {
        self.observation_diagnostics.borrow().last().cloned()
    }

    pub(crate) fn take_durable_run(&mut self) -> DurableReceiverRun {
        std::mem::replace(&mut self.durable_run, DurableReceiverRun::Idle)
    }

    pub(crate) fn store_durable_run(&mut self, run: DurableReceiverRun) {
        self.durable_run = run;
    }

    pub(crate) fn has_answer_controller_cleanup_capacity(&self) -> bool {
        self.answer_controller_cleanups.len() < MAX_ANSWER_CONTROLLER_CLEANUPS
    }

    pub(crate) fn push_answer_controller_cleanup(
        &mut self,
        cleanup: ReceiverAnswerControllerCleanup,
    ) {
        assert!(self.has_answer_controller_cleanup_capacity());
        self.answer_controller_cleanups.push_back(cleanup);
    }

    pub(crate) fn take_answer_controller_cleanup(
        &mut self,
    ) -> Option<ReceiverAnswerControllerCleanup> {
        self.answer_controller_cleanups.pop_front()
    }

    pub(crate) fn answer_controller_cleanup_count(&self) -> usize {
        self.answer_controller_cleanups.len()
    }

    #[cfg(test)]
    pub(crate) fn pending_answer_controller_cleanups(&self) -> usize {
        self.answer_controller_cleanup_count()
    }

    #[cfg(test)]
    pub(crate) fn active_durable_run(&self) -> Option<&super::ActiveReceiverRun> {
        match &self.durable_run {
            DurableReceiverRun::Active(active) => Some(active),
            DurableReceiverRun::Idle
            | DurableReceiverRun::Claimed(_)
            | DurableReceiverRun::RecoveryClaimed(_)
            | DurableReceiverRun::RecoveryPreSpawnCleanup(_)
            | DurableReceiverRun::RecoverySpawned(_)
            | DurableReceiverRun::AnswerCleanupPending(_)
            | DurableReceiverRun::CleanupPending(_) => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn recovery_claimed_durable_run(&self) -> Option<&super::ClaimedReceiverRun> {
        match &self.durable_run {
            DurableReceiverRun::RecoveryClaimed(claimed) => Some(claimed),
            DurableReceiverRun::Idle
            | DurableReceiverRun::Claimed(_)
            | DurableReceiverRun::RecoveryPreSpawnCleanup(_)
            | DurableReceiverRun::RecoverySpawned(_)
            | DurableReceiverRun::Active(_)
            | DurableReceiverRun::AnswerCleanupPending(_)
            | DurableReceiverRun::CleanupPending(_) => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn spawned_recovery_durable_run(&self) -> Option<&super::SpawnedRecoveryRun> {
        match &self.durable_run {
            DurableReceiverRun::RecoverySpawned(spawned) => Some(spawned),
            DurableReceiverRun::Idle
            | DurableReceiverRun::Claimed(_)
            | DurableReceiverRun::RecoveryClaimed(_)
            | DurableReceiverRun::RecoveryPreSpawnCleanup(_)
            | DurableReceiverRun::Active(_)
            | DurableReceiverRun::AnswerCleanupPending(_)
            | DurableReceiverRun::CleanupPending(_) => None,
        }
    }

    #[must_use]
    pub(crate) const fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn record_intent(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    #[must_use]
    pub(crate) const fn sync_gate_is_armed(&self) -> bool {
        self.sync_gate.is_some()
    }
}
