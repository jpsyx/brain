//! Pure control transitions plus authoritative workspace registration checks.

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use super::{ControlRequest, ControlResponse, LeaseRegistration, ServerSnapshot};
use crate::server::lifecycle::{
    LEASE_TTL, LeaseAction, LeaseTable, ServerDecision, ServerGeneration, WorkspaceLease,
};
use crate::workspace::{RegistryStore, WorkspaceManifest, WorkspaceName};

/// Generation-bound shared-server control state.
#[derive(Debug)]
pub struct ControlServer {
    generation: ServerGeneration,
    registry_store: RegistryStore,
    runtime_home: PathBuf,
    leases: LeaseTable,
}

/// Nonblocking owner of the machine-global control socket.
#[derive(Debug)]
pub struct ControlListener {
    listener: UnixListener,
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
        Ok(Self { listener })
    }

    /// Drain every ready request and return the strongest lifecycle decision.
    ///
    /// Malformed client frames receive a rejection and do not terminate the
    /// process. Accept failures are returned to the process loop.
    ///
    /// # Errors
    ///
    /// Returns an error when accepting the next ready local connection fails.
    pub fn drain(&self, server: &mut ControlServer) -> Result<ServerDecision> {
        loop {
            match self.listener.accept() {
                Ok((mut stream, _)) => match handle_stream(&mut stream, server) {
                    Ok(ServerDecision::ShutdownNow) => {
                        return Ok(ServerDecision::ShutdownNow);
                    }
                    Ok(ServerDecision::KeepRunning) => {}
                    Err(error) => crate::logging::log(format!(
                        "shared-server control request failed: {error:#}"
                    )),
                },
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    return Ok(ServerDecision::KeepRunning);
                }
                Err(error) => return Err(error).context("accepting server control request"),
            }
        }
    }
}

const STREAM_TIMEOUT: Duration = Duration::from_secs(2);

fn handle_stream(stream: &mut UnixStream, server: &mut ControlServer) -> Result<ServerDecision> {
    let deadline = Instant::now()
        .checked_add(STREAM_TIMEOUT)
        .context("server control timeout exceeds the monotonic clock range")?;
    let response = match super::codec::read_until(stream, deadline) {
        Ok(request) => server.apply_until(request, Instant::now(), deadline),
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
        }
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
            Err(error) => ControlResponse::Rejected {
                message: error.to_string(),
            },
        }
    }

    /// Mutable lease state used by the process watchdog and later routing.
    pub(crate) const fn leases_mut(&mut self) -> &mut LeaseTable {
        &mut self.leases
    }

    /// Resolve an HTTP ingress through current live leases before loading any
    /// workspace-specific state.
    pub(crate) fn resolve_workspace_route(
        &mut self,
        ingress: crate::server::IngressId,
        now: Instant,
    ) -> Result<
        crate::server::workspace_route::ResolvedWorkspaceRoute,
        crate::server::workspace_route::WorkspaceRouteError,
    > {
        crate::server::workspace_route::WorkspaceRouteResolver::new(
            &mut self.leases,
            &self.registry_store,
            &self.runtime_home,
            now,
        )
        .resolve(ingress)
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
            ControlRequest::UpdateEnabled {
                lease_id,
                receiver_enabled,
                ..
            } => {
                let decision = self.leases.apply(LeaseAction::SetReceiverEnabled {
                    lease_id,
                    receiver_enabled,
                    now,
                })?;
                ControlOutcome::Decision(decision)
            }
            ControlRequest::Unregister { lease_id, .. } => {
                let decision = self
                    .leases
                    .apply(LeaseAction::Unregister { lease_id, now })?;
                ControlOutcome::Decision(decision)
            }
            ControlRequest::Snapshot => {
                let live_leases = self.leases.live_workspaces(now).len();
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
        let registry = RegistryStore::load_from(self.registry_store.path())
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
            crate::workspace::WorkspacePaths::new(&self.runtime_home, registration.workspace_id);
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
}
