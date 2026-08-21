#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AcquisitionStage {
    WorkspaceSingleton,
    ReceiverEndpoint,
    ServerLease,
    Terminal,
    Application,
    BackgroundServices,
}

const ACQUISITION_ORDER: [AcquisitionStage; 6] = [
    AcquisitionStage::WorkspaceSingleton,
    AcquisitionStage::ReceiverEndpoint,
    AcquisitionStage::ServerLease,
    AcquisitionStage::Terminal,
    AcquisitionStage::Application,
    AcquisitionStage::BackgroundServices,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ShutdownStage {
    StopServerLease,
    ShutdownAgentControllers,
    StopPeriodicPuller,
    StopWatcher,
    ReleaseSessionLock,
    RestoreTerminal,
}

const SHUTDOWN_ORDER: [ShutdownStage; 6] = [
    ShutdownStage::StopServerLease,
    ShutdownStage::ShutdownAgentControllers,
    ShutdownStage::StopPeriodicPuller,
    ShutdownStage::StopWatcher,
    ShutdownStage::ReleaseSessionLock,
    ShutdownStage::RestoreTerminal,
];

pub(super) struct RuntimeLifecycle {
    acquired: usize,
    shutdown_started: bool,
    singleton_held: bool,
}

impl RuntimeLifecycle {
    pub(super) const fn new() -> Self {
        Self {
            acquired: 0,
            shutdown_started: false,
            singleton_held: false,
        }
    }

    pub(super) fn record_acquired(&mut self, stage: AcquisitionStage) -> anyhow::Result<()> {
        let expected = ACQUISITION_ORDER.get(self.acquired).copied();
        anyhow::ensure!(
            expected == Some(stage),
            "runtime acquired {stage:?} while {} was next",
            expected.map_or_else(|| "no resource".to_owned(), |next| format!("{next:?}"))
        );
        self.acquired += 1;
        if stage == AcquisitionStage::WorkspaceSingleton {
            self.singleton_held = true;
        }
        Ok(())
    }

    pub(super) const fn is_running(&self) -> bool {
        self.acquired == ACQUISITION_ORDER.len() && !self.shutdown_started
    }

    pub(super) fn begin_shutdown(&mut self) -> Vec<ShutdownStage> {
        if self.shutdown_started {
            return Vec::new();
        }
        self.shutdown_started = true;
        SHUTDOWN_ORDER.to_vec()
    }

    pub(super) const fn singleton_held(&self) -> bool {
        self.singleton_held
    }

    pub(super) const fn begin_drop(&mut self) {
        self.singleton_held = false;
    }
}

#[cfg(test)]
mod tests {
    use super::{AcquisitionStage, RuntimeLifecycle, ShutdownStage};

    fn running_lifecycle() -> RuntimeLifecycle {
        let mut lifecycle = RuntimeLifecycle::new();
        for stage in [
            AcquisitionStage::WorkspaceSingleton,
            AcquisitionStage::ReceiverEndpoint,
            AcquisitionStage::ServerLease,
            AcquisitionStage::Terminal,
            AcquisitionStage::Application,
            AcquisitionStage::BackgroundServices,
        ] {
            lifecycle.record_acquired(stage).unwrap();
        }
        lifecycle
    }

    #[test]
    fn runtime_acquisition_state_requires_the_startup_resource_order() {
        let mut lifecycle = RuntimeLifecycle::new();

        let error = lifecycle
            .record_acquired(AcquisitionStage::ServerLease)
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "runtime acquired ServerLease while WorkspaceSingleton was next"
        );
        assert!(!lifecycle.is_running());
    }

    #[test]
    fn orderly_shutdown_preserves_resource_order_and_runs_only_once() {
        let mut lifecycle = running_lifecycle();

        assert_eq!(
            lifecycle.begin_shutdown(),
            vec![
                ShutdownStage::StopServerLease,
                ShutdownStage::ShutdownAgentControllers,
                ShutdownStage::StopPeriodicPuller,
                ShutdownStage::StopWatcher,
                ShutdownStage::ReleaseSessionLock,
                ShutdownStage::RestoreTerminal,
            ]
        );
        assert!(lifecycle.begin_shutdown().is_empty());
    }

    #[test]
    fn singleton_remains_owned_through_orderly_shutdown_until_runtime_drop() {
        let mut lifecycle = running_lifecycle();

        let _ = lifecycle.begin_shutdown();

        assert!(lifecycle.singleton_held());
        lifecycle.begin_drop();
        assert!(!lifecycle.singleton_held());
    }
}
