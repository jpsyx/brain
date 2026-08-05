//! Bounded client transport for the machine-wide shared-server control socket.

use std::fs::{self, OpenOptions};
use std::net::Shutdown;
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use super::{ControlRequest, ControlResponse, LeaseRegistration, codec};
use crate::server::lifecycle::{
    LeaseId, ProcessRecord, ServerDecision, ServerGeneration, ServerPaths, connect_or_elect_until,
};
use crate::workspace::WorkspaceId;

/// Maximum time one local request may spend reading or writing its frame.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
const REGISTRATION_TIMEOUT: Duration = Duration::from_secs(4);

/// Injected synchronization point after process discovery and before register.
pub trait RegistrationGate: Send + 'static {
    /// Observe a selected generation before its registration request is sent.
    fn after_connect(&mut self, record: &ProcessRecord);
}

struct ImmediateRegistration;

impl RegistrationGate for ImmediateRegistration {
    fn after_connect(&mut self, _record: &ProcessRecord) {}
}

/// Reachability and control client for the machine-wide shared server.
#[derive(Debug, Clone)]
pub struct ServerClient {
    paths: ServerPaths,
    executable: Option<PathBuf>,
    home: Option<PathBuf>,
}

impl ServerClient {
    /// Target one explicit shared-server directory.
    #[must_use]
    pub const fn new(paths: ServerPaths) -> Self {
        Self {
            paths,
            executable: None,
            home: None,
        }
    }

    /// Target explicit paths and a specific executable for crash recovery.
    ///
    /// This is primarily useful to exercise the real election path from an
    /// integration-test process whose current executable is the test harness.
    #[must_use]
    pub const fn with_launch_context(
        paths: ServerPaths,
        executable: PathBuf,
        home: PathBuf,
    ) -> Self {
        Self {
            paths,
            executable: Some(executable),
            home: Some(home),
        }
    }

    /// Paths targeted by this client.
    #[must_use]
    pub const fn paths(&self) -> &ServerPaths {
        &self.paths
    }

    /// Register one generation-tagged TUI lease.
    ///
    /// # Errors
    ///
    /// Returns an error when the server is unavailable, stale, or rejects it.
    pub fn register_generation(&self, registration: &LeaseRegistration) -> Result<ServerDecision> {
        response_decision(self.request(&ControlRequest::Register(registration.clone()))?)
    }

    /// Connect or elect, then register within one bounded startup handshake.
    ///
    /// Authoritative registration rejection returns immediately. Missing or
    /// stale transport and process generations re-enter election while time
    /// remains.
    ///
    /// # Errors
    ///
    /// Returns an error on timeout, authoritative rejection, or invalid wire
    /// response.
    pub fn connect_and_register(
        &self,
        registration: &mut LeaseRegistration,
    ) -> Result<ProcessRecord> {
        self.connect_and_register_with_gate(registration, ImmediateRegistration)
    }

    /// Registration handshake with an injected post-connect race boundary.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::connect_and_register`].
    pub fn connect_and_register_with_gate(
        &self,
        registration: &mut LeaseRegistration,
        mut gate: impl RegistrationGate,
    ) -> Result<ProcessRecord> {
        let deadline = Instant::now()
            .checked_add(REGISTRATION_TIMEOUT)
            .context("server registration timeout exceeds the monotonic clock range")?;
        loop {
            let record = connect_or_elect_until(self, deadline)?;
            registration.generation = record.generation;
            gate.after_connect(&record);
            match self.request_until(&ControlRequest::Register(registration.clone()), deadline) {
                Ok(ControlResponse::Accepted {
                    shutdown: false, ..
                }) => return Ok(record),
                Ok(ControlResponse::StaleGeneration) => {}
                Ok(ControlResponse::Rejected { message }) => {
                    anyhow::bail!("shared brain server rejected request: {message}");
                }
                Ok(response) => {
                    anyhow::bail!("unexpected shared-server registration response: {response:?}");
                }
                Err(error) if is_recoverable_registration_transport(&error) => {}
                Err(error) => return Err(error),
            }
            if Instant::now() >= deadline {
                anyhow::bail!("shared brain server registration deadline elapsed");
            }
        }
    }

    /// Renew one lease against its current process generation.
    ///
    /// # Errors
    ///
    /// Returns an error when transport or lease validation fails.
    pub fn heartbeat(
        &self,
        generation: ServerGeneration,
        lease_id: LeaseId,
    ) -> Result<ServerDecision> {
        response_decision(self.request(&ControlRequest::Heartbeat {
            generation,
            lease_id,
        })?)
    }

    /// Reload persistent receiver intent into one live workspace lease.
    ///
    /// # Errors
    ///
    /// Returns an error when no shared process exists or rejects the refresh.
    pub fn refresh_enabled(&self, workspace_id: WorkspaceId) -> Result<ServerDecision> {
        let generation = self.connect_existing()?.generation;
        self.refresh_enabled_generation(generation, workspace_id)
    }

    /// Reload persistent receiver intent against an already observed process.
    ///
    /// # Errors
    ///
    /// Returns an error when the generation is stale or rejects the refresh.
    pub fn refresh_enabled_generation(
        &self,
        generation: ServerGeneration,
        workspace_id: WorkspaceId,
    ) -> Result<ServerDecision> {
        response_decision(self.request(&ControlRequest::RefreshEnabled {
            generation,
            workspace_id,
        })?)
    }

    /// Unregister one lease against an explicit generation.
    ///
    /// # Errors
    ///
    /// Returns an error when transport or lease validation fails.
    pub fn unregister_generation(
        &self,
        generation: ServerGeneration,
        lease_id: LeaseId,
    ) -> Result<ServerDecision> {
        response_decision(self.request(&ControlRequest::Unregister {
            generation,
            lease_id,
        })?)
    }

    /// Unregister one lease from the currently published generation.
    ///
    /// # Errors
    ///
    /// Returns an error when connection or unregister fails.
    pub fn unregister(&self, lease_id: LeaseId) -> Result<ServerDecision> {
        let generation = self.connect_existing()?.generation;
        self.unregister_generation(generation, lease_id)
    }

    /// Exchange one bounded newline-delimited request and response.
    ///
    /// # Errors
    ///
    /// Returns an error when the socket is unavailable, times out, or violates
    /// the protocol.
    pub fn request(&self, request: &ControlRequest) -> Result<ControlResponse> {
        self.request_with_timeout(request, REQUEST_TIMEOUT)
    }

    /// Exchange one request within one total connect, write, and read budget.
    ///
    /// # Errors
    ///
    /// Returns an error when the timeout is zero or any transport phase fails.
    pub fn request_with_timeout(
        &self,
        request: &ControlRequest,
        timeout: Duration,
    ) -> Result<ControlResponse> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .context("server control request timeout exceeds the monotonic clock range")?;
        self.request_until(request, deadline)
    }

    pub(super) fn request_until(
        &self,
        request: &ControlRequest,
        deadline: Instant,
    ) -> Result<ControlResponse> {
        let mut stream = self.connect_until(deadline)?;
        codec::write_until(&mut stream, request, deadline)?;
        stream.shutdown(Shutdown::Write).ok();
        codec::read_until(&mut stream, deadline).context("reading shared-server response")
    }

    fn connect_until(&self, deadline: Instant) -> Result<UnixStream> {
        super::connect::connect_until(&self.paths.control_socket(), deadline)
            .context("connecting to the shared brain server")
    }

    pub(crate) fn spawn(&self, generation: ServerGeneration, port: u16) -> Result<u32> {
        use std::os::unix::process::CommandExt as _;

        fs::create_dir_all(self.paths.directory())
            .with_context(|| format!("creating {}", self.paths.directory().display()))?;
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.paths.log())
            .with_context(|| format!("opening {}", self.paths.log().display()))?;
        fs::set_permissions(self.paths.log(), fs::Permissions::from_mode(0o600))
            .with_context(|| format!("securing {}", self.paths.log().display()))?;
        let error_log = log.try_clone().context("cloning server log handle")?;
        let executable = self.executable.clone().map_or_else(
            || std::env::current_exe().context("resolving the brain executable"),
            Ok,
        )?;
        let mut command = Command::new(executable);
        command.args([
            "server",
            "run",
            "--generation",
            &generation.to_string(),
            "--port",
            &port.to_string(),
        ]);
        if let Some(home) = &self.home {
            command.env("HOME", home);
        }
        let child = command
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(error_log))
            .process_group(0)
            .spawn()
            .context("spawning elected shared brain server")?;
        Ok(child.id())
    }
}

fn is_recoverable_registration_transport(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause.downcast_ref::<std::io::Error>().is_some_and(|io| {
            matches!(
                io.kind(),
                std::io::ErrorKind::NotFound
                    | std::io::ErrorKind::ConnectionRefused
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::WouldBlock
                    | std::io::ErrorKind::UnexpectedEof
            )
        })
    })
}

impl Default for ServerClient {
    fn default() -> Self {
        Self::new(ServerPaths::default())
    }
}

fn response_decision(response: ControlResponse) -> Result<ServerDecision> {
    match response {
        ControlResponse::Accepted { shutdown, .. } => Ok(if shutdown {
            ServerDecision::ShutdownNow
        } else {
            ServerDecision::KeepRunning
        }),
        ControlResponse::StaleGeneration => {
            anyhow::bail!("shared brain server generation is stale")
        }
        ControlResponse::Rejected { message } => {
            anyhow::bail!("shared brain server rejected request: {message}")
        }
        ControlResponse::Snapshot(_) => anyhow::bail!("unexpected shared-server snapshot response"),
        ControlResponse::WorkspaceIngress { .. } => {
            anyhow::bail!("unexpected shared-server workspace ingress response")
        }
        ControlResponse::WorkspaceStatus { .. } => {
            anyhow::bail!("unexpected shared-server workspace status response")
        }
    }
}
