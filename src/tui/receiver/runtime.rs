//! Durable receiver scheduling state and the legacy endpoint lifetime owner.

use std::time::Instant;

use super::DurableReceiverRun;

mod sync;

pub(crate) use sync::{SyncGateObservation, SyncGatePoll};

struct ReceiverSyncGate {
    seen_journal_id: Option<i64>,
    launched_at: Instant,
    next_poll: Instant,
    attempts: u8,
}

pub(crate) struct ReceiverRuntime {
    #[allow(dead_code)] // BR-18 removes this builder-compatible endpoint owner.
    legacy_job_socket: Option<crate::tui::singleton::JobSocket>,
    enabled: bool,
    sync_gate: Option<ReceiverSyncGate>,
    durable_run: DurableReceiverRun,
    #[cfg(test)]
    after_restart_scan_hook: Option<Box<dyn FnOnce()>>,
}

impl ReceiverRuntime {
    #[must_use]
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            legacy_job_socket: None,
            enabled,
            sync_gate: None,
            durable_run: DurableReceiverRun::Idle,
            #[cfg(test)]
            after_restart_scan_hook: None,
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

    pub(crate) fn take_durable_run(&mut self) -> DurableReceiverRun {
        std::mem::replace(&mut self.durable_run, DurableReceiverRun::Idle)
    }

    pub(crate) fn store_durable_run(&mut self, run: DurableReceiverRun) {
        self.durable_run = run;
    }

    #[cfg(test)]
    pub(crate) fn active_durable_run(&self) -> Option<&super::ActiveReceiverRun> {
        match &self.durable_run {
            DurableReceiverRun::Active(active) => Some(active),
            DurableReceiverRun::Idle | DurableReceiverRun::Claimed(_) => None,
        }
    }

    pub(crate) fn install_legacy_job_socket(&mut self, socket: crate::tui::singleton::JobSocket) {
        self.legacy_job_socket = Some(socket);
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
