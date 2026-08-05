//! Bounded client transport for the machine-wide shared-server control socket.

use std::fs::{self, OpenOptions};
use std::net::Shutdown;
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result};

use super::{ControlRequest, ControlResponse, LeaseRegistration, codec};
use crate::server::lifecycle::{
    LeaseId, ProcessRecord, ServerDecision, ServerGeneration, ServerPaths, WorkspaceLease,
    pid_alive,
};

/// Maximum time one local request may spend reading or writing its frame.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);

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

    /// Connect to the published generation without electing or spawning.
    ///
    /// # Errors
    ///
    /// Returns an error when no matching live process and control socket exist.
    pub fn connect_existing(&self) -> Result<ProcessRecord> {
        let record = crate::server::lifecycle::read_record(&self.paths)
            .context("brain server is not running; open a brain TUI first")?;
        if !pid_alive(record.pid) {
            anyhow::bail!("brain server process {} is not alive", record.pid);
        }
        match self.request(&ControlRequest::Snapshot)? {
            ControlResponse::Snapshot(snapshot) if snapshot.generation == record.generation => {
                Ok(record)
            }
            ControlResponse::Snapshot(_) => {
                anyhow::bail!("brain server generation changed while connecting")
            }
            response => anyhow::bail!("unexpected shared-server status response: {response:?}"),
        }
    }

    /// Register one generation-tagged TUI lease.
    ///
    /// # Errors
    ///
    /// Returns an error when the server is unavailable, stale, or rejects it.
    pub fn register_generation(&self, registration: &LeaseRegistration) -> Result<ServerDecision> {
        response_decision(self.request(&ControlRequest::Register(registration.clone()))?)
    }

    /// Compatibility wrapper for callers that already hold a complete lease.
    ///
    /// # Errors
    ///
    /// Returns an error when connection or registration fails.
    pub fn register(&self, lease: &WorkspaceLease) -> Result<ServerDecision> {
        let generation = self.connect_existing()?.generation;
        self.register_generation(&LeaseRegistration {
            generation,
            lease_id: lease.lease_id,
            workspace_id: lease.workspace_id,
            canonical_name: lease.canonical_name.to_string(),
            ingress_id: lease.ingress_id,
            tui_pid: lease.tui_pid,
            job_socket: lease.job_socket.clone(),
        })
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

    /// Update receiver intent for one live lease.
    ///
    /// # Errors
    ///
    /// Returns an error when transport or lease validation fails.
    pub fn update_enabled(
        &self,
        generation: ServerGeneration,
        lease_id: LeaseId,
        receiver_enabled: bool,
    ) -> Result<ServerDecision> {
        response_decision(self.request(&ControlRequest::UpdateEnabled {
            generation,
            lease_id,
            receiver_enabled,
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
        let mut stream = UnixStream::connect(self.paths.control_socket())
            .context("connecting to the shared brain server")?;
        stream
            .set_read_timeout(Some(REQUEST_TIMEOUT))
            .context("setting server control read timeout")?;
        stream
            .set_write_timeout(Some(REQUEST_TIMEOUT))
            .context("setting server control write timeout")?;
        codec::write(&mut stream, request)?;
        stream.shutdown(Shutdown::Write).ok();
        codec::read(&mut stream).context("reading shared-server response")
    }

    pub(crate) fn spawn(&self, generation: ServerGeneration, port: u16) -> Result<()> {
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
        command
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(error_log))
            .process_group(0)
            .spawn()
            .context("spawning elected shared brain server")?;
        Ok(())
    }
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
    }
}
