use super::{ReceiverRuntime, ReceiverSyncGate};

pub(crate) enum SyncGatePoll {
    Waiting,
    Completed,
    Retry(u8),
    Exhausted,
}

#[derive(Clone, Copy)]
pub(crate) struct SyncGateObservation {
    now: std::time::Instant,
    latest_journal_id: Option<i64>,
    sync_running: bool,
}

impl SyncGateObservation {
    #[must_use]
    pub(crate) const fn new(
        now: std::time::Instant,
        latest_journal_id: Option<i64>,
        sync_running: bool,
    ) -> Self {
        Self {
            now,
            latest_journal_id,
            sync_running,
        }
    }
}

impl ReceiverRuntime {
    pub(crate) fn arm_sync_gate(
        &mut self,
        now: std::time::Instant,
        seen_journal_id: Option<i64>,
        attempts: u8,
    ) {
        self.sync_gate = Some(ReceiverSyncGate {
            seen_journal_id,
            launched_at: now,
            next_poll: now,
            attempts,
        });
    }

    #[must_use]
    pub(crate) fn poll_sync_gate(
        &mut self,
        observation: SyncGateObservation,
    ) -> Option<SyncGatePoll> {
        let mut gate = self.sync_gate.take()?;
        if observation.now < gate.next_poll {
            self.sync_gate = Some(gate);
            return Some(SyncGatePoll::Waiting);
        }
        if journal_advanced(gate.seen_journal_id, observation.latest_journal_id) {
            return Some(SyncGatePoll::Completed);
        }
        if observation.sync_running
            || observation.now.duration_since(gate.launched_at)
                < crate::sync::freshness::SYNC_START_GRACE
        {
            gate.next_poll = observation.now + crate::sync::freshness::STATUS_POLL_INTERVAL;
            self.sync_gate = Some(gate);
            return Some(SyncGatePoll::Waiting);
        }
        if gate.attempts >= crate::sync::freshness::MAX_PULL_LAUNCH_ATTEMPTS {
            return Some(SyncGatePoll::Exhausted);
        }
        Some(SyncGatePoll::Retry(gate.attempts.saturating_add(1)))
    }
}

fn journal_advanced(seen: Option<i64>, latest: Option<i64>) -> bool {
    match (seen, latest) {
        (Some(seen), Some(latest)) => latest > seen,
        (None, Some(_)) => true,
        _ => false,
    }
}
