//! Pure control transitions plus authoritative workspace registration checks.

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::{UnixListener, UnixStream};
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
                Ok((mut stream, _)) => {
                    if handle_stream(&mut stream, server) == ServerDecision::ShutdownNow {
                        return Ok(ServerDecision::ShutdownNow);
                    }
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

fn handle_stream(stream: &mut UnixStream, server: &mut ControlServer) -> ServerDecision {
    let _ = stream.set_read_timeout(Some(STREAM_TIMEOUT));
    let _ = stream.set_write_timeout(Some(STREAM_TIMEOUT));
    let response = match super::codec::read(stream) {
        Ok(request) => server.apply(request, Instant::now()),
        Err(error) => ControlResponse::Rejected {
            message: error.to_string(),
        },
    };
    let decision = match response {
        ControlResponse::Accepted { shutdown: true, .. } => ServerDecision::ShutdownNow,
        _ => ServerDecision::KeepRunning,
    };
    let _ = super::codec::write(stream, &response);
    decision
}

impl ControlServer {
    /// Create an empty control state for one process generation.
    #[must_use]
    pub fn new(generation: ServerGeneration, registry_store: RegistryStore) -> Self {
        Self {
            generation,
            registry_store,
            leases: LeaseTable::default(),
        }
    }

    /// Apply one request without performing socket I/O.
    #[must_use]
    pub fn apply(&mut self, request: ControlRequest, now: Instant) -> ControlResponse {
        if request
            .generation()
            .is_some_and(|generation| generation != self.generation)
        {
            return ControlResponse::StaleGeneration;
        }

        match self.apply_current(request, now) {
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

    fn apply_current(&mut self, request: ControlRequest, now: Instant) -> Result<ControlOutcome> {
        let outcome = match request {
            ControlRequest::Register(registration) => {
                let lease = self.validate_registration(&registration, now)?;
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
        let expires_at = now
            .checked_add(LEASE_TTL)
            .context("lease expiry exceeds the monotonic clock range")?;
        Ok(WorkspaceLease {
            lease_id: registration.lease_id,
            workspace_id: registration.workspace_id,
            canonical_name: WorkspaceName::parse(&registration.canonical_name)?,
            ingress_id: registration.ingress_id,
            tui_pid: registration.tui_pid,
            job_socket: registration.job_socket.clone(),
            receiver_enabled: record.receiver_enabled,
            expires_at,
        })
    }
}

enum ControlOutcome {
    Decision(ServerDecision),
    Snapshot(ServerSnapshot),
}
