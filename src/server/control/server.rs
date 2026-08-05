//! Pure control transitions plus authoritative workspace registration checks.

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use super::{ControlRequest, ControlResponse, LeaseRegistration, ServerSnapshot};
use crate::server::lifecycle::{
    LEASE_TTL, LeaseAction, LeaseTable, ServerDecision, ServerGeneration, WorkspaceLease,
};
use crate::workspace::{RegistryStore, WorkspaceManifest, WorkspaceName};

/// Generation-bound shared-server control state.
pub struct ControlServer {
    generation: ServerGeneration,
    registry_store: RegistryStore,
    runtime_home: PathBuf,
    leases: LeaseTable,
    admissions: Vec<std::sync::Weak<crate::server::receiver::admission::ReceiverAdmission>>,
    #[cfg(test)]
    io_gate: Option<Arc<dyn Fn() + Send + Sync>>,
}

/// Nonblocking owner of the machine-global control socket.
#[derive(Debug)]
pub struct ControlListener {
    listener: UnixListener,
    shutdown: Arc<AtomicBool>,
}

impl ControlListener {
    /// Bind and secure the control socket for one elected process.
    ///
    /// # Errors
    ///
    /// Returns an error when the socket cannot be bound, secured, or configured.
    pub fn bind(paths: &crate::server::lifecycle::ServerPaths) -> Result<Self> {
        let listener = UnixListener::bind(paths.control_socket())
            .with_context(|| format!("binding {}", paths.control_socket().display()))?;
        fs::set_permissions(paths.control_socket(), fs::Permissions::from_mode(0o600))
            .with_context(|| format!("securing {}", paths.control_socket().display()))?;
        listener
            .set_nonblocking(true)
            .context("making server control socket nonblocking")?;
        Ok(Self {
            listener,
            shutdown: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Drain every ready request and return the strongest lifecycle decision.
    ///
    /// Malformed client frames receive a rejection and do not terminate the
    /// process. Accept failures are returned to the process loop.
    ///
    /// # Errors
    ///
    /// Returns an error when accepting the next ready local connection fails.
    pub fn drain(&self, server: &Arc<Mutex<ControlServer>>) -> Result<ServerDecision> {
        if self.shutdown.load(Ordering::Acquire) {
            return Ok(ServerDecision::ShutdownNow);
        }
        loop {
            match self.listener.accept() {
                Ok((mut stream, _)) => {
                    let server = Arc::clone(server);
                    let shutdown = Arc::clone(&self.shutdown);
                    std::thread::Builder::new()
                        .name("brain-server-control".to_owned())
                        .spawn(move || match handle_stream(&mut stream, &server) {
                            Ok(ServerDecision::ShutdownNow) => {
                                shutdown.store(true, Ordering::Release);
                            }
                            Ok(ServerDecision::KeepRunning) => {}
                            Err(error) => crate::logging::log(format!(
                                "shared-server control request failed: {error:#}"
                            )),
                        })
                        .context("starting shared-server control worker")?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    return Ok(ServerDecision::KeepRunning);
                }
                Err(error) => return Err(error).context("accepting server control request"),
            }
        }
    }
}

const STREAM_TIMEOUT: Duration = Duration::from_secs(2);

fn handle_stream(
    stream: &mut UnixStream,
    server: &Arc<Mutex<ControlServer>>,
) -> Result<ServerDecision> {
    let deadline = Instant::now()
        .checked_add(STREAM_TIMEOUT)
        .context("server control timeout exceeds the monotonic clock range")?;
    let response = match super::codec::read_until(stream, deadline) {
        Ok(request) => ControlServer::apply_shared_until(server, request, Instant::now(), deadline),
        Err(error) => ControlResponse::Rejected {
            message: error.to_string(),
        },
    };
    let decision = match response {
        ControlResponse::Accepted { shutdown: true, .. } => ServerDecision::ShutdownNow,
        _ => ServerDecision::KeepRunning,
    };
    super::codec::write_until(stream, &response, deadline)?;
    Ok(decision)
}

impl ControlServer {
    fn apply_shared_until(
        shared: &Arc<Mutex<Self>>,
        request: ControlRequest,
        now: Instant,
        deadline: Instant,
    ) -> ControlResponse {
        Self::apply_shared_until_with_clock(shared, request, now, deadline, &Instant::now)
    }

    fn apply_shared_until_with_clock(
        shared: &Arc<Mutex<Self>>,
        request: ControlRequest,
        now: Instant,
        deadline: Instant,
        clock: &impl Fn() -> Instant,
    ) -> ControlResponse {
        let (generation, registry_store, runtime_home) = {
            let server = shared
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if request
                .generation()
                .is_some_and(|candidate| candidate != server.generation)
            {
                return ControlResponse::StaleGeneration;
            }
            (
                server.generation,
                server.registry_store.clone(),
                server.runtime_home.clone(),
            )
        };
        #[cfg(test)]
        let io_gate = shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .io_gate
            .clone();
        let prepared = match &request {
            ControlRequest::Register(registration) => {
                if clock() >= deadline {
                    return deadline_rejection();
                }
                #[cfg(test)]
                if let Some(gate) = &io_gate {
                    gate();
                }
                match validate_registration_with(
                    &registry_store,
                    &runtime_home,
                    registration,
                    now,
                    deadline,
                ) {
                    Ok(lease) => Some(PreparedControl::Register(lease)),
                    Err(error) => {
                        return ControlResponse::Rejected {
                            message: error.to_string(),
                        };
                    }
                }
            }
            ControlRequest::RefreshEnabled { workspace_id, .. } => {
                if clock() >= deadline {
                    return deadline_rejection();
                }
                #[cfg(test)]
                if let Some(gate) = &io_gate {
                    gate();
                }
                let result = RegistryStore::load_from(registry_store.path())
                    .context("reopening receiver intent from the machine workspace registry")
                    .and_then(|registry| {
                        registry
                            .workspaces
                            .values()
                            .find(|record| record.workspace_id == *workspace_id)
                            .map(|record| record.receiver_enabled)
                            .context("receiver workspace no longer exists in the machine registry")
                    });
                match result {
                    Ok(enabled) => Some(PreparedControl::Refresh(*workspace_id, enabled)),
                    Err(error) => {
                        return ControlResponse::Rejected {
                            message: error.to_string(),
                        };
                    }
                }
            }
            _ => None,
        };
        if clock() >= deadline {
            return deadline_rejection();
        }
        let revocations = {
            let mut server = shared
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match &prepared {
                Some(PreparedControl::Refresh(workspace_id, false)) => {
                    server.admissions_for_workspace(*workspace_id)
                }
                _ => match &request {
                    ControlRequest::Unregister { lease_id, .. } => {
                        server.admissions_for_lease(*lease_id)
                    }
                    _ => Vec::new(),
                },
            }
        };
        for admission in revocations {
            admission.revoke_or_wait();
        }
        if clock() >= deadline {
            return deadline_rejection();
        }
        let mut server = shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if server.generation != generation {
            return ControlResponse::StaleGeneration;
        }
        if clock() >= deadline {
            return deadline_rejection();
        }
        match prepared {
            Some(PreparedControl::Register(lease)) => {
                match server.leases.apply(LeaseAction::Register { lease, now }) {
                    Ok(decision) => server.decision_response(decision),
                    Err(error) => ControlResponse::Rejected {
                        message: error.to_string(),
                    },
                }
            }
            Some(PreparedControl::Refresh(workspace_id, enabled)) => {
                match server
                    .leases
                    .refresh_workspace_receiver_enabled(workspace_id, enabled, now)
                {
                    Ok(()) => server.decision_response(ServerDecision::KeepRunning),
                    Err(error) => ControlResponse::Rejected {
                        message: error.to_string(),
                    },
                }
            }
            None => server.apply_until(request, now, deadline),
        }
    }

    const fn decision_response(&self, decision: ServerDecision) -> ControlResponse {
        ControlResponse::Accepted {
            generation: self.generation,
            shutdown: matches!(decision, ServerDecision::ShutdownNow),
        }
    }
    /// Create an empty control state for one process generation.
    #[must_use]
    pub fn new(
        generation: ServerGeneration,
        registry_store: RegistryStore,
        runtime_home: PathBuf,
    ) -> Self {
        Self {
            generation,
            registry_store,
            runtime_home,
            leases: LeaseTable::default(),
            admissions: Vec::new(),
            #[cfg(test)]
            io_gate: None,
        }
    }

    #[cfg(test)]
    fn set_io_gate(&mut self, gate: Arc<dyn Fn() + Send + Sync>) {
        self.io_gate = Some(gate);
    }

    /// Apply one request without performing socket I/O.
    #[must_use]
    pub fn apply(&mut self, request: ControlRequest, now: Instant) -> ControlResponse {
        let Some(deadline) = Instant::now().checked_add(STREAM_TIMEOUT) else {
            return ControlResponse::Rejected {
                message: "server control timeout exceeds the monotonic clock range".to_owned(),
            };
        };
        self.apply_until(request, now, deadline)
    }

    /// Apply one request within the control connection's absolute deadline.
    #[must_use]
    pub fn apply_until(
        &mut self,
        request: ControlRequest,
        now: Instant,
        deadline: Instant,
    ) -> ControlResponse {
        if request
            .generation()
            .is_some_and(|generation| generation != self.generation)
        {
            return ControlResponse::StaleGeneration;
        }

        match self.apply_current(request, now, deadline) {
            Ok(ControlOutcome::Decision(decision)) => ControlResponse::Accepted {
                generation: self.generation,
                shutdown: matches!(decision, ServerDecision::ShutdownNow),
            },
            Ok(ControlOutcome::Snapshot(snapshot)) => ControlResponse::Snapshot(snapshot),
            Ok(ControlOutcome::WorkspaceIngress(route)) => {
                let (ingress_id, lease_id) = route.map_or((None, None), |(ingress, lease)| {
                    (Some(ingress), Some(lease))
                });
                ControlResponse::WorkspaceIngress {
                    generation: self.generation,
                    ingress_id,
                    lease_id,
                }
            }
            Ok(ControlOutcome::WorkspaceStatus(status)) => ControlResponse::WorkspaceStatus {
                generation: self.generation,
                live_leases: status.live_leases,
                receiver_enabled: status.receiver_enabled,
            },
            Err(error) => ControlResponse::Rejected {
                message: error.to_string(),
            },
        }
    }

    /// Mutable lease state used by the process watchdog and later routing.
    pub(crate) const fn leases_mut(&mut self) -> &mut LeaseTable {
        &mut self.leases
    }

    /// Capture route authority and a filesystem loader without doing IO.
    pub(crate) fn begin_workspace_route(
        &mut self,
        ingress: crate::server::IngressId,
        now: Instant,
    ) -> Result<
        (
            crate::server::workspace_route::WorkspaceRouteTicket,
            crate::server::workspace_route::VerifiedWorkspaceContextLoader,
        ),
        crate::server::workspace_route::WorkspaceRouteError,
    > {
        let ticket = crate::server::workspace_route::WorkspaceRouteAuthority::begin(
            &mut self.leases,
            self.generation,
            ingress,
            now,
        )?;
        let loader = crate::server::workspace_route::VerifiedWorkspaceContextLoader::new(
            self.registry_store.clone(),
            self.runtime_home.clone(),
        );
        Ok((ticket, loader))
    }

    pub(crate) fn begin_local_workspace_route(
        &mut self,
        ingress: crate::server::IngressId,
        capability: crate::server::lifecycle::LeaseId,
        now: Instant,
    ) -> Result<
        (
            crate::server::workspace_route::WorkspaceRouteTicket,
            crate::server::workspace_route::VerifiedWorkspaceContextLoader,
        ),
        crate::server::workspace_route::WorkspaceRouteError,
    > {
        let ticket = crate::server::workspace_route::WorkspaceRouteAuthority::begin_local(
            &mut self.leases,
            self.generation,
            ingress,
            capability,
            now,
        )?;
        let loader = crate::server::workspace_route::VerifiedWorkspaceContextLoader::new(
            self.registry_store.clone(),
            self.runtime_home.clone(),
        );
        Ok((ticket, loader))
    }

    /// Revalidate captured route authority after filesystem loading.
    pub(crate) fn finish_workspace_route(
        &mut self,
        ticket: &crate::server::workspace_route::WorkspaceRouteTicket,
        context: crate::workspace::WorkspaceContext,
        now: Instant,
    ) -> Result<
        crate::server::workspace_route::ResolvedWorkspaceRoute,
        crate::server::workspace_route::WorkspaceRouteError,
    > {
        crate::server::workspace_route::WorkspaceRouteAuthority::finish(
            &mut self.leases,
            self.generation,
            ticket,
            now,
        )?;
        Ok(
            crate::server::workspace_route::ResolvedWorkspaceRoute::with_authority(
                context,
                ticket.lease().clone(),
                self.registry_store.clone(),
                ticket.clone(),
            ),
        )
    }

    /// Revalidate one resolved route immediately before receiver handoff.
    pub(crate) fn revalidate_workspace_route(
        &mut self,
        route: &crate::server::workspace_route::ResolvedWorkspaceRoute,
        now: Instant,
    ) -> Result<(), crate::server::workspace_route::WorkspaceRouteError> {
        crate::server::workspace_route::WorkspaceRouteAuthority::finish(
            &mut self.leases,
            self.generation,
            route.authority_ticket()?,
            now,
        )
    }

    pub(crate) fn begin_receiver_admission(
        &mut self,
        route: &crate::server::workspace_route::ResolvedWorkspaceRoute,
        now: Instant,
    ) -> Result<
        Arc<crate::server::receiver::admission::ReceiverAdmission>,
        crate::server::workspace_route::WorkspaceRouteError,
    > {
        self.revalidate_workspace_route(route, now)?;
        let admission = Arc::new(crate::server::receiver::admission::ReceiverAdmission::new(
            route.lease().workspace_id,
            route.lease().lease_id,
        ));
        self.admissions
            .retain(|candidate| candidate.strong_count() > 0);
        self.admissions.push(Arc::downgrade(&admission));
        Ok(admission)
    }

    fn admissions_for_workspace(
        &mut self,
        workspace_id: crate::workspace::WorkspaceId,
    ) -> Vec<Arc<crate::server::receiver::admission::ReceiverAdmission>> {
        let mut matches = Vec::new();
        self.admissions.retain(|candidate| {
            let Some(admission) = candidate.upgrade() else {
                return false;
            };
            if admission.workspace_id() == workspace_id {
                matches.push(admission);
            }
            true
        });
        matches
    }

    fn admissions_for_lease(
        &mut self,
        lease_id: crate::server::lifecycle::LeaseId,
    ) -> Vec<Arc<crate::server::receiver::admission::ReceiverAdmission>> {
        let mut matches = Vec::new();
        self.admissions.retain(|candidate| {
            let Some(admission) = candidate.upgrade() else {
                return false;
            };
            if admission.lease_id() == lease_id {
                matches.push(admission);
            }
            true
        });
        matches
    }

    fn apply_current(
        &mut self,
        request: ControlRequest,
        now: Instant,
        deadline: Instant,
    ) -> Result<ControlOutcome> {
        let outcome = match request {
            ControlRequest::Register(registration) => {
                let lease = self.validate_registration(&registration, now, deadline)?;
                let decision = self.leases.apply(LeaseAction::Register { lease, now })?;
                ControlOutcome::Decision(decision)
            }
            ControlRequest::Heartbeat { lease_id, .. } => {
                let decision = self.leases.apply(LeaseAction::Heartbeat {
                    lease_id,
                    now,
                    timing: crate::server::lifecycle::LeaseTiming::PRODUCTION,
                })?;
                ControlOutcome::Decision(decision)
            }
            ControlRequest::RefreshEnabled { workspace_id, .. } => {
                let registry = RegistryStore::load_from(self.registry_store.path())
                    .context("reopening receiver intent from the machine workspace registry")?;
                let receiver_enabled = registry
                    .workspaces
                    .values()
                    .find(|record| record.workspace_id == workspace_id)
                    .context("receiver workspace no longer exists in the machine registry")?
                    .receiver_enabled;
                if !receiver_enabled {
                    for admission in self.admissions_for_workspace(workspace_id) {
                        admission.revoke_or_wait();
                    }
                }
                self.leases.refresh_workspace_receiver_enabled(
                    workspace_id,
                    receiver_enabled,
                    now,
                )?;
                ControlOutcome::Decision(ServerDecision::KeepRunning)
            }
            ControlRequest::Unregister { lease_id, .. } => {
                for admission in self.admissions_for_lease(lease_id) {
                    admission.revoke_or_wait();
                }
                let decision = self
                    .leases
                    .apply(LeaseAction::Unregister { lease_id, now })?;
                ControlOutcome::Decision(decision)
            }
            ControlRequest::WorkspaceIngress { workspace_id, .. } => {
                ControlOutcome::WorkspaceIngress(self.leases.live_local_route(workspace_id, now))
            }
            ControlRequest::WorkspaceStatus { workspace_id, .. } => {
                ControlOutcome::WorkspaceStatus(self.leases.status_view(workspace_id, now))
            }
            ControlRequest::Snapshot => {
                let live_leases = self.leases.live_count_at(now);
                ControlOutcome::Snapshot(ServerSnapshot {
                    generation: self.generation,
                    live_leases,
                })
            }
        };
        Ok(outcome)
    }

    fn validate_registration(
        &self,
        registration: &LeaseRegistration,
        now: Instant,
        deadline: Instant,
    ) -> Result<WorkspaceLease> {
        validate_registration_with(
            &self.registry_store,
            &self.runtime_home,
            registration,
            now,
            deadline,
        )
    }
}

enum PreparedControl {
    Register(WorkspaceLease),
    Refresh(crate::workspace::WorkspaceId, bool),
}

fn deadline_rejection() -> ControlResponse {
    ControlResponse::Rejected {
        message: "shared-server control request deadline elapsed".to_owned(),
    }
}

fn validate_registration_with(
    registry_store: &RegistryStore,
    runtime_home: &Path,
    registration: &LeaseRegistration,
    now: Instant,
    deadline: Instant,
) -> Result<WorkspaceLease> {
    let registry = RegistryStore::load_from(registry_store.path())
        .context("reopening the machine workspace registry")?;
    let selected = registry
        .select(Some(&registration.canonical_name))
        .context("selecting the registered canonical workspace")?;
    if selected.canonical_name().as_str() != registration.canonical_name {
        anyhow::bail!("workspace registration must use its canonical name");
    }
    let record = selected.record();
    if record.workspace_id != registration.workspace_id {
        anyhow::bail!("workspace registration UUID does not match the machine registry");
    }
    let authoritative_root = crate::workspace::normalize_root(&record.root, Path::new("/"))?;
    let resolved_root =
        crate::workspace::normalize_root(&registration.resolved_root, Path::new("/"))?;
    if authoritative_root != resolved_root {
        anyhow::bail!("workspace root changed after the TUI resolved it");
    }
    let manifest = WorkspaceManifest::load(&record.root, env!("CARGO_PKG_VERSION"))
        .context("reopening the registered workspace manifest")?;
    if manifest.workspace_id() != registration.workspace_id {
        anyhow::bail!("workspace manifest UUID does not match the machine registry");
    }
    if crate::server::lifecycle::IngressId::from(manifest.receiver_ingress_id())
        != registration.ingress_id
    {
        anyhow::bail!("workspace ingress UUID does not match its manifest");
    }
    let runtime_paths =
        crate::workspace::WorkspacePaths::new(runtime_home, registration.workspace_id);
    let expected_job_socket = runtime_paths.job_socket();
    if registration.job_socket != expected_job_socket {
        anyhow::bail!("job socket does not match the validated workspace");
    }
    validate_live_tui(&runtime_paths, registration.tui_pid, deadline)?;
    let expires_at = now
        .checked_add(LEASE_TTL)
        .context("lease expiry exceeds the monotonic clock range")?;
    Ok(WorkspaceLease {
        lease_id: registration.lease_id,
        workspace_id: registration.workspace_id,
        canonical_name: WorkspaceName::parse(&registration.canonical_name)?,
        ingress_id: registration.ingress_id,
        tui_pid: registration.tui_pid,
        job_socket: expected_job_socket,
        receiver_enabled: record.receiver_enabled,
        expires_at,
    })
}

fn validate_live_tui(
    runtime_paths: &crate::workspace::WorkspacePaths,
    expected_pid: u32,
    deadline: Instant,
) -> Result<()> {
    let lock_pid = fs::read_to_string(runtime_paths.tui_lock())
        .context("reading the workspace TUI singleton")?
        .trim()
        .parse::<u32>()
        .context("parsing the workspace TUI singleton PID")?;
    if lock_pid != expected_pid || !crate::server::lifecycle::pid_alive(expected_pid) {
        anyhow::bail!("workspace TUI singleton does not match a live process");
    }
    super::connect::connect_until(&runtime_paths.job_socket(), deadline)
        .context("connecting to the live workspace job listener")?;
    Ok(())
}

enum ControlOutcome {
    Decision(ServerDecision),
    Snapshot(ServerSnapshot),
    WorkspaceIngress(Option<(crate::server::IngressId, crate::server::lifecycle::LeaseId)>),
    WorkspaceStatus(crate::server::lifecycle::LeaseStatusView),
}

#[cfg(test)]
mod tests;
