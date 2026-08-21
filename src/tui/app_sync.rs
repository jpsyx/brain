//! TUI sync observability and the receiver's downstream-freshness gate.

use crate::sync::args::Direction;
use crate::sync::config::SyncConfig;
use crate::sync::journal::Journal;
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
        let now = self.receiver_sync_runtime.monotonic_now();
        if now < self.sync_status_next_poll {
            return;
        }
        self.sync_status_next_poll = now + crate::sync::freshness::STATUS_POLL_INTERVAL;
        self.sync_status = self
            .receiver_sync_runtime
            .live_sync_state(self.command_context.workspace.paths())
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
        if self.sync_status.is_none() && self.receiver.sync_gate_is_armed() {
            self.sync_status = Some("↻ preparing receiver message sync…".to_owned());
        }
        let latest = self
            .receiver_sync_runtime
            .latest_successful_downstream_id(self.command_context.workspace.paths());
        if journal_advanced(self.last_seen_downstream_id, latest) {
            self.last_seen_downstream_id = latest;
            if let Err(error) = self.reload_tasks() {
                crate::logging::log(format!("post-sync task refresh failed: {error:#}"));
                self.flash = Some(FlashKind::Error(format!(
                    "post-sync task refresh failed: {error}"
                )));
            }
        }
    }

    pub(crate) fn receiver_sync_ready(&mut self) -> bool {
        let config = SyncConfig::load(&self.command_context);
        if !config.is_configured() {
            return true;
        }

        if self.receiver.sync_gate_is_armed() {
            let workspace = std::sync::Arc::clone(&self.command_context.workspace);
            let observation = crate::tui::receiver::SyncGateObservation::new(
                self.receiver_sync_runtime.monotonic_now(),
                self.receiver_sync_runtime
                    .latest_successful_downstream_id(workspace.paths()),
                self.receiver_sync_runtime
                    .live_sync_state(workspace.paths())
                    .is_some(),
            );
            let poll = self
                .receiver
                .poll_sync_gate(observation)
                .expect("an armed receiver sync gate must accept an observation");
            match poll {
                crate::tui::receiver::SyncGatePoll::Waiting => return false,
                crate::tui::receiver::SyncGatePoll::Completed => {
                    crate::logging::log("receiver freshness pull completed; dispatch may continue");
                    self.sync_status = None;
                    let _ = self.reload_tasks();
                    return true;
                }
                crate::tui::receiver::SyncGatePoll::Exhausted => {
                    crate::logging::log(
                        "receiver freshness pull did not start after three attempts; dispatching with local state",
                    );
                    self.sync_status = None;
                    self.flash = Some(FlashKind::Error(
                        "receiver sync could not start; processing with local brain state"
                            .to_owned(),
                    ));
                    return true;
                }
                crate::tui::receiver::SyncGatePoll::Retry(attempts) => {
                    return self.launch_receiver_pull(attempts);
                }
            }
        }

        if let Some(state) = self
            .receiver_sync_runtime
            .live_sync_state(self.command_context.workspace.paths())
        {
            if state.direction != "push" {
                self.arm_receiver_sync_gate(0);
            }
            return false;
        }

        let last_downstream = self
            .receiver_sync_runtime
            .latest_downstream_completion(self.command_context.workspace.paths());
        if !crate::sync::freshness::message_pull_due(
            last_downstream.as_deref(),
            self.receiver_sync_runtime.utc_now(),
        ) {
            return true;
        }
        self.launch_receiver_pull(1)
    }

    fn launch_receiver_pull(&mut self, attempts: u8) -> bool {
        crate::logging::log(format!(
            "receiver message waiting for downstream freshness pull attempt={attempts}"
        ));
        if self
            .receiver_sync_runtime
            .spawn_detached_sync(&self.command_context.workspace, Direction::Pull)
            .is_none()
        {
            self.flash = Some(FlashKind::Error(
                "receiver sync could not start; processing with local brain state".to_owned(),
            ));
            return true;
        }
        self.arm_receiver_sync_gate(attempts);
        false
    }

    fn arm_receiver_sync_gate(&mut self, attempts: u8) {
        let now = self.receiver_sync_runtime.monotonic_now();
        let seen_journal_id = self
            .receiver_sync_runtime
            .latest_successful_downstream_id(self.command_context.workspace.paths());
        self.receiver.arm_sync_gate(now, seen_journal_id, attempts);
        self.sync_status = Some("↻ preparing receiver message sync…".to_owned());
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
