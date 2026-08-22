//! TUI sync observability and the receiver's downstream-freshness gate.

use crate::sync::args::Direction;
use crate::sync::config::SyncConfig;
use crate::sync::journal::Journal;
use crate::tui::receiver::ReceiverEffectOutcome;
use crate::tui::{App, FlashKind};

pub(crate) trait ReceiverSyncRuntime: Send {
    fn monotonic_now(&self) -> std::time::Instant;
    fn utc_now(&self) -> chrono::DateTime<chrono::Utc>;
    fn live_sync_state(
        &self,
        paths: &crate::workspace::WorkspacePaths,
    ) -> Option<crate::sync::current::CurrentState>;
    fn latest_successful_downstream_id(
        &self,
        paths: &crate::workspace::WorkspacePaths,
    ) -> Option<i64>;
    fn latest_downstream_completion(
        &self,
        paths: &crate::workspace::WorkspacePaths,
    ) -> Option<String>;
    fn spawn_detached_sync(
        &self,
        workspace: &crate::workspace::WorkspaceContext,
        direction: Direction,
    ) -> Option<u32>;
}

pub(crate) struct SystemReceiverSyncRuntime;

impl ReceiverSyncRuntime for SystemReceiverSyncRuntime {
    fn monotonic_now(&self) -> std::time::Instant {
        std::time::Instant::now()
    }

    fn utc_now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now()
    }

    fn live_sync_state(
        &self,
        paths: &crate::workspace::WorkspacePaths,
    ) -> Option<crate::sync::current::CurrentState> {
        crate::sync::current::read_state(paths)
            .filter(|state| crate::server::lifecycle::pid_alive(state.pid))
    }

    fn latest_successful_downstream_id(
        &self,
        paths: &crate::workspace::WorkspacePaths,
    ) -> Option<i64> {
        Journal::open(&paths.sync_journal())
            .ok()
            .and_then(|journal| journal.latest_successful_downstream_id().ok())
            .flatten()
    }

    fn latest_downstream_completion(
        &self,
        paths: &crate::workspace::WorkspacePaths,
    ) -> Option<String> {
        Journal::open(&paths.sync_journal())
            .ok()
            .and_then(|journal| journal.latest_downstream_completion().ok())
            .flatten()
    }

    fn spawn_detached_sync(
        &self,
        workspace: &crate::workspace::WorkspaceContext,
        direction: Direction,
    ) -> Option<u32> {
        crate::sync::trigger::spawn_detached_sync(workspace, direction)
    }
}

impl App {
    pub(crate) fn tick_sync_status(&mut self) {
        let now = self.services.monotonic_now();
        if !self.status.sync_poll_due(now) {
            return;
        }
        self.status.schedule_next_sync_poll(now);
        let sync_status = self
            .services
            .live_sync_state(self.context.workspace().paths())
            .map(|state| {
                if self.receiver.sync_gate_is_armed() {
                    format!(
                        "↻ syncing brain before receiver message ({})…",
                        state.direction
                    )
                } else {
                    format!("↻ syncing brain ({})…", state.direction)
                }
            });
        self.status.set_sync_status(sync_status);
        if self.status.sync_status().is_none() && self.receiver.sync_gate_is_armed() {
            self.status
                .set_sync_status(Some("↻ preparing receiver message sync…".to_owned()));
        }
        let latest = self
            .services
            .latest_successful_downstream_id(self.context.workspace().paths());
        if journal_advanced(self.status.last_seen_downstream_id(), latest) {
            self.status.record_downstream_id(latest);
            if let Err(error) = self.reload_tasks() {
                crate::logging::log(format!("post-sync task refresh failed: {error:#}"));
                self.status.set_flash(FlashKind::Error(format!(
                    "post-sync task refresh failed: {error}"
                )));
            }
        }
    }

    pub(crate) fn execute_receiver_sync_freshness_effect(&mut self) -> ReceiverEffectOutcome {
        let config = SyncConfig::load(self.context.command());
        if !config.is_configured() {
            return ReceiverEffectOutcome::Completed;
        }

        if self.receiver.sync_gate_is_armed() {
            let workspace = std::sync::Arc::clone(&self.context.command().workspace);
            let observation = crate::tui::receiver::SyncGateObservation::new(
                self.services.monotonic_now(),
                self.services
                    .latest_successful_downstream_id(workspace.paths()),
                self.services.live_sync_state(workspace.paths()).is_some(),
            );
            let poll = self
                .receiver
                .poll_sync_gate(observation)
                .expect("an armed receiver sync gate must accept an observation");
            match poll {
                crate::tui::receiver::SyncGatePoll::Waiting => {
                    return ReceiverEffectOutcome::FreshnessPending;
                }
                crate::tui::receiver::SyncGatePoll::Completed => {
                    crate::logging::log("receiver freshness pull completed; dispatch may continue");
                    self.status.set_sync_status(None);
                    let _ = self.reload_tasks();
                    return ReceiverEffectOutcome::Completed;
                }
                crate::tui::receiver::SyncGatePoll::Exhausted => {
                    crate::logging::log(
                        "receiver freshness pull did not start after three attempts; dispatching with local state",
                    );
                    self.status.set_sync_status(None);
                    self.status.set_flash(FlashKind::Error(
                        "receiver sync could not start; processing with local brain state"
                            .to_owned(),
                    ));
                    return ReceiverEffectOutcome::Completed;
                }
                crate::tui::receiver::SyncGatePoll::Retry(attempts) => {
                    return self.launch_receiver_pull(attempts);
                }
            }
        }

        if let Some(state) = self
            .services
            .live_sync_state(self.context.workspace().paths())
        {
            if state.direction != "push" {
                self.arm_receiver_sync_gate(0);
            }
            return ReceiverEffectOutcome::FreshnessPending;
        }

        let last_downstream = self
            .services
            .latest_downstream_completion(self.context.workspace().paths());
        if !crate::sync::freshness::message_pull_due(
            last_downstream.as_deref(),
            self.services.utc_now(),
        ) {
            return ReceiverEffectOutcome::Completed;
        }
        self.launch_receiver_pull(1)
    }

    fn launch_receiver_pull(&mut self, attempts: u8) -> ReceiverEffectOutcome {
        crate::logging::log(format!(
            "receiver message waiting for downstream freshness pull attempt={attempts}"
        ));
        if self
            .services
            .spawn_detached_sync(self.context.workspace(), Direction::Pull)
            .is_none()
        {
            self.status.set_flash(FlashKind::Error(
                "receiver sync could not start; processing with local brain state".to_owned(),
            ));
            return ReceiverEffectOutcome::Completed;
        }
        self.arm_receiver_sync_gate(attempts);
        ReceiverEffectOutcome::FreshnessPending
    }

    fn arm_receiver_sync_gate(&mut self, attempts: u8) {
        let now = self.services.monotonic_now();
        let seen_journal_id = self
            .services
            .latest_successful_downstream_id(self.context.workspace().paths());
        self.receiver.arm_sync_gate(now, seen_journal_id, attempts);
        self.status
            .set_sync_status(Some("↻ preparing receiver message sync…".to_owned()));
    }
}

fn journal_advanced(seen: Option<i64>, latest: Option<i64>) -> bool {
    match (seen, latest) {
        (Some(seen), Some(latest)) => latest > seen,
        (None, Some(_)) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::journal_advanced;

    #[test]
    fn receiver_gate_only_opens_after_a_new_sync_run_is_recorded() {
        assert!(!journal_advanced(None, None));
        assert!(journal_advanced(None, Some(1)));
        assert!(!journal_advanced(Some(4), Some(4)));
        assert!(journal_advanced(Some(4), Some(5)));
    }
}
