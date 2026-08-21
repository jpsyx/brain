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

pub(super) struct StartupResources<ServerLease, JobSocket> {
    // Rust drops fields in declaration order, so unregister precedes socket removal.
    server_lease: ServerLease,
    job_socket: JobSocket,
}

impl<ServerLease, JobSocket> StartupResources<ServerLease, JobSocket> {
    pub(super) const fn new(server_lease: ServerLease, job_socket: JobSocket) -> Self {
        Self {
            server_lease,
            job_socket,
        }
    }

    pub(super) fn prepare<Prepared>(
        self,
        prepare: impl FnOnce(&ServerLease) -> anyhow::Result<Prepared>,
    ) -> anyhow::Result<PreparedStartup<Prepared, ServerLease, JobSocket>> {
        let prepared = prepare(&self.server_lease)?;
        let Self {
            server_lease,
            job_socket,
        } = self;
        Ok(PreparedStartup {
            prepared,
            server_lease,
            job_socket,
        })
    }
}

pub(super) struct PreparedStartup<Prepared, ServerLease, JobSocket> {
    prepared: Prepared,
    // Prepared state unwinds first, then the lease, then the socket.
    server_lease: ServerLease,
    job_socket: JobSocket,
}

impl<Prepared, ServerLease, JobSocket> PreparedStartup<Prepared, ServerLease, JobSocket> {
    pub(super) fn finish<Runtime>(
        self,
        finish: impl FnOnce(Prepared, ServerLease, JobSocket) -> Runtime,
    ) -> Runtime {
        let Self {
            prepared,
            server_lease,
            job_socket,
        } = self;
        finish(prepared, server_lease, job_socket)
    }
}

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
    use std::sync::{Arc, Mutex};

    use super::{AcquisitionStage, RuntimeLifecycle, ShutdownStage, StartupResources};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum PartialStartupDrop {
        ServerLease,
        JobSocket,
    }

    struct RecordedResource {
        event: PartialStartupDrop,
        events: Arc<Mutex<Vec<PartialStartupDrop>>>,
    }

    impl RecordedResource {
        fn new(event: PartialStartupDrop, events: &Arc<Mutex<Vec<PartialStartupDrop>>>) -> Self {
            Self {
                event,
                events: Arc::clone(events),
            }
        }
    }

    impl Drop for RecordedResource {
        fn drop(&mut self) {
            self.events.lock().unwrap().push(self.event);
        }
    }

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

    #[test]
    fn application_setup_failure_drops_server_lease_before_job_socket() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let resources = StartupResources::new(
            RecordedResource::new(PartialStartupDrop::ServerLease, &events),
            RecordedResource::new(PartialStartupDrop::JobSocket, &events),
        );

        let result: anyhow::Result<()> = resources
            .prepare(|_| -> anyhow::Result<()> {
                anyhow::bail!("injected application setup failure")
            })
            .map(|_| ());

        assert_eq!(
            result.unwrap_err().to_string(),
            "injected application setup failure"
        );
        assert_eq!(
            *events.lock().unwrap(),
            vec![
                PartialStartupDrop::ServerLease,
                PartialStartupDrop::JobSocket,
            ]
        );
    }
}
