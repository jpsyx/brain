use crate::sync::args::Direction;

use super::{ReceiverRuntime, ReceiverSyncGate};

pub(crate) enum SyncGatePoll {
    Waiting,
    Completed,
    Retry(u8),
    Exhausted,
}

impl ReceiverRuntime {
    #[must_use]
    pub(crate) fn utc_now(&self) -> chrono::DateTime<chrono::Utc> {
        self.sync_runtime.utc_now()
    }

    #[must_use]
    pub(crate) fn live_sync_state(
        &self,
        paths: &crate::workspace::WorkspacePaths,
    ) -> Option<crate::sync::current::CurrentState> {
        self.sync_runtime.live_sync_state(paths)
    }

    #[must_use]
    pub(crate) fn latest_downstream_completion(
        &self,
        paths: &crate::workspace::WorkspacePaths,
    ) -> Option<String> {
        self.sync_runtime.latest_downstream_completion(paths)
    }

    #[must_use]
    pub(crate) fn spawn_detached_sync(
        &self,
        workspace: &crate::workspace::WorkspaceContext,
        direction: Direction,
    ) -> Option<u32> {
        self.sync_runtime.spawn_detached_sync(workspace, direction)
    }

    pub(crate) fn arm_sync_gate(&mut self, paths: &crate::workspace::WorkspacePaths, attempts: u8) {
        let now = self.monotonic_now();
        self.sync_gate = Some(ReceiverSyncGate {
            seen_journal_id: self.latest_successful_downstream_id(paths),
            launched_at: now,
            next_poll: now,
            attempts,
        });
    }

    #[must_use]
    pub(crate) fn poll_sync_gate(
        &mut self,
        paths: &crate::workspace::WorkspacePaths,
    ) -> Option<SyncGatePoll> {
        let mut gate = self.sync_gate.take()?;
        let now = self.monotonic_now();
        if now < gate.next_poll {
            self.sync_gate = Some(gate);
            return Some(SyncGatePoll::Waiting);
        }
        if journal_advanced(
            gate.seen_journal_id,
            self.latest_successful_downstream_id(paths),
        ) {
            return Some(SyncGatePoll::Completed);
        }
        if self.live_sync_state(paths).is_some()
            || now.duration_since(gate.launched_at) < crate::sync::freshness::SYNC_START_GRACE
        {
            gate.next_poll = now + crate::sync::freshness::STATUS_POLL_INTERVAL;
            self.sync_gate = Some(gate);
            return Some(SyncGatePoll::Waiting);
        }
        if gate.attempts >= crate::sync::freshness::MAX_PULL_LAUNCH_ATTEMPTS {
            return Some(SyncGatePoll::Exhausted);
        }
        Some(SyncGatePoll::Retry(gate.attempts.saturating_add(1)))
    }

    #[cfg(test)]
    pub(crate) fn replace_sync_runtime(
        &mut self,
        runtime: Box<dyn crate::tui::ReceiverSyncRuntime>,
    ) {
        self.sync_runtime = runtime;
    }
}

fn journal_advanced(seen: Option<i64>, latest: Option<i64>) -> bool {
    match (seen, latest) {
        (Some(seen), Some(latest)) => latest > seen,
        (None, Some(_)) => true,
        _ => false,
    }
}
