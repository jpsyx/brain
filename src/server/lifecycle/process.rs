//! Thin process, control-socket, and client shells around lifecycle decisions.

use std::fs::{self, OpenOptions};
use std::io::{Read as _, Write as _};
use std::net::{Ipv4Addr, TcpListener};
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tiny_http::Server;

use super::{
    ElectionGuard, IngressId, LeaseAction, LeaseId, LeaseTable, ProcessRecord, ServerDecision,
    ServerGeneration, ServerPaths, StartDecision, WorkspaceLease, decide_start, pid_alive,
    watchdog::Watchdog,
};
use crate::theme::Theme;
use crate::workspace::{WorkspaceId, WorkspaceName};

const PREFERRED_PORT: u16 = 8787;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(2);
const INITIAL_REGISTRATION_TIMEOUT: Duration = Duration::from_secs(2);
const POLL_INTERVAL: Duration = Duration::from_millis(25);
const STREAM_TIMEOUT: Duration = Duration::from_secs(2);

/// Reachability and lifecycle client for the machine-wide shared server.
#[derive(Debug, Clone)]
pub struct ServerClient {
    paths: ServerPaths,
}

impl ServerClient {
    /// Target one explicit shared-server directory.
    #[must_use]
    pub const fn new(paths: ServerPaths) -> Self {
        Self { paths }
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
        let record = super::state::read_record(&self.paths)
            .context("brain server is not running; open a brain TUI first")?;
        if !pid_alive(record.pid) {
            anyhow::bail!("brain server process {} is not alive", record.pid);
        }
        let response = self.send(&ControlRequest::Ping)?;
        if response.generation != record.generation {
            anyhow::bail!("brain server generation changed while connecting");
        }
        Ok(record)
    }

    /// Register one verified live TUI lease through the narrow Task 2 seam.
    ///
    /// # Errors
    ///
    /// Returns an error when the server is unavailable or rejects the lease.
    pub fn register(&self, lease: &WorkspaceLease) -> Result<ServerDecision> {
        let response = self.send(&ControlRequest::Register {
            lease_id: lease.lease_id,
            workspace_id: lease.workspace_id,
            canonical_name: lease.canonical_name.to_string(),
            ingress_id: lease.ingress_id,
            tui_pid: lease.tui_pid,
            job_socket: lease.job_socket.clone(),
            receiver_enabled: lease.receiver_enabled,
        })?;
        Ok(response.decision())
    }

    /// Unregister one orderly TUI lease.
    ///
    /// # Errors
    ///
    /// Returns an error when the server is unavailable or rejects the frame.
    pub fn unregister(&self, lease_id: LeaseId) -> Result<ServerDecision> {
        Ok(self
            .send(&ControlRequest::Unregister { lease_id })?
            .decision())
    }

    fn send(&self, request: &ControlRequest) -> Result<ControlResponse> {
        let mut stream = UnixStream::connect(self.paths.control_socket())
            .context("connecting to the shared brain server")?;
        stream
            .set_read_timeout(Some(STREAM_TIMEOUT))
            .context("setting server control read timeout")?;
        stream
            .set_write_timeout(Some(STREAM_TIMEOUT))
            .context("setting server control write timeout")?;
        serde_json::to_writer(&mut stream, request).context("writing server control request")?;
        stream.shutdown(std::net::Shutdown::Write).ok();
        let mut bytes = Vec::new();
        stream
            .read_to_end(&mut bytes)
            .context("reading server control response")?;
        let response: ControlResponse =
            serde_json::from_slice(&bytes).context("parsing server control response")?;
        if let Some(error) = response.error.as_deref() {
            anyhow::bail!("shared brain server rejected request: {error}");
        }
        Ok(response)
    }

    fn spawn(&self, generation: ServerGeneration, port: u16) -> Result<()> {
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
        let executable = std::env::current_exe().context("resolving the brain executable")?;
        Command::new(executable)
            .args([
                "server",
                "run",
                "--generation",
                &generation.to_string(),
                "--port",
                &port.to_string(),
            ])
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

/// Report current process reachability without starting or cleaning anything.
pub fn status() -> Result<()> {
    let theme = Theme::active();
    let mut output = std::io::stdout().lock();
    match ServerClient::default().connect_existing() {
        Ok(record) => writeln!(
            output,
            "{}",
            theme.success(&format!(
                "✓ brain server running (pid {}, port {}, generation {})",
                record.pid, record.port, record.generation
            ))
        )?,
        Err(_) => writeln!(output, "{}", theme.muted("brain server is not running"))?,
    }
    Ok(())
}

/// Print the machine-wide shared-process lifecycle log without starting it.
pub fn logs() -> Result<()> {
    let theme = Theme::active();
    let paths = ServerPaths::default();
    let mut output = std::io::stdout().lock();
    match fs::read_to_string(paths.log()) {
        Ok(contents) => write!(output, "{contents}")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            writeln!(output, "{}", theme.muted("no brain server log yet"))?;
        }
        Err(error) => {
            return Err(error).with_context(|| format!("reading {}", paths.log().display()));
        }
    }
    Ok(())
}

/// Connect to the shared process or elect exactly one detached starter.
///
/// Only long-lived TUI startup and crash recovery may call this function.
/// Short-lived callers must use [`ServerClient::connect_existing`].
///
/// # Errors
///
/// Returns an error when election, spawning, or bounded startup fails.
pub fn connect_or_elect(client: &ServerClient) -> Result<ProcessRecord> {
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    loop {
        let record = super::state::read_record(client.paths());
        let process_live = record.as_ref().is_some_and(|state| pid_alive(state.pid));
        let socket_live = record
            .as_ref()
            .is_some_and(|state| client.connect_existing().is_ok_and(|found| found == *state));
        if process_live && socket_live {
            return Ok(record.expect("live probes require a process record"));
        }

        let generation = ServerGeneration::new();
        let election = ElectionGuard::try_acquire(client.paths(), generation)?;
        match decide_start(
            record.as_ref(),
            process_live,
            socket_live,
            election.is_some(),
        ) {
            StartDecision::Reuse(record) => return Ok(record),
            StartDecision::Start { remove_stale_state } => {
                let guard = election.expect("start decision requires election ownership");
                if remove_stale_state {
                    if let Some(stale) = record {
                        super::state::remove_generation(client.paths(), stale.generation)?;
                    }
                } else {
                    remove_stale_socket(client.paths())?;
                }
                let port = choose_port(preferred_is_free(PREFERRED_PORT), PREFERRED_PORT);
                client.spawn(guard.generation(), port)?;
                let handoff = guard.handoff();
                if let Some(found) = wait_for_connection(client, deadline)? {
                    drop(handoff);
                    return Ok(found);
                }
                drop(handoff);
            }
            StartDecision::WaitForWinner => {
                if let Some(found) = wait_for_connection(client, deadline)? {
                    return Ok(found);
                }
            }
        }
        if Instant::now() >= deadline {
            anyhow::bail!("brain server did not come up within {STARTUP_TIMEOUT:?}");
        }
    }
}

fn wait_for_connection(client: &ServerClient, deadline: Instant) -> Result<Option<ProcessRecord>> {
    loop {
        if let Ok(record) = client.connect_existing() {
            return Ok(Some(record));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// Pick the preferred port when free, otherwise request an ephemeral port.
#[must_use]
pub const fn choose_port(preferred_free: bool, preferred: u16) -> u16 {
    if preferred_free { preferred } else { 0 }
}

fn preferred_is_free(preferred: u16) -> bool {
    TcpListener::bind((Ipv4Addr::LOCALHOST, preferred)).is_ok()
}

fn remove_stale_socket(paths: &ServerPaths) -> Result<()> {
    match fs::remove_file(paths.control_socket()) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).context("removing stale server control socket"),
    }
}

/// Run the token-guarded shared server process until final lease shutdown.
///
/// # Errors
///
/// Returns an error when election validation, binding, state publication, or
/// the process loop fails.
pub fn run_process(paths: &ServerPaths, generation: ServerGeneration, port: u16) -> Result<()> {
    let election = ElectionGuard::adopt(paths, generation)?;
    let _owner = ProcessOwner {
        paths: paths.clone(),
        generation,
        _election: election,
    };
    let terminate = termination_flag()?;
    let control = bind_control(paths)?;
    let server = Server::http(("127.0.0.1", port))
        .map_err(|error| anyhow::anyhow!("binding 127.0.0.1:{port}: {error}"))?;
    let actual_port = server
        .server_addr()
        .to_ip()
        .context("resolving the bound server address")?
        .port();
    let record = ProcessRecord {
        pid: std::process::id(),
        port: actual_port,
        generation,
        started_at: chrono::Utc::now().to_rfc3339(),
    };
    super::state::write_record(paths, &record)?;
    wait_at_test_startup_gate()?;
    append_log(
        paths,
        &format!("server generation {generation} started on port {actual_port}"),
    );
    let mut leases = LeaseTable::default();
    let watchdog = Watchdog::new(Instant::now(), INITIAL_REGISTRATION_TIMEOUT);

    while !terminate.load(Ordering::Relaxed) {
        if drain_control(&control, generation, &mut leases)? == ServerDecision::ShutdownNow {
            break;
        }
        if watchdog.tick(&mut leases, Instant::now())? == ServerDecision::ShutdownNow {
            break;
        }
        if let Some(mut request) = server.recv_timeout(POLL_INTERVAL)? {
            let response = super::super::respond(&mut request);
            let _ = request.respond(response);
        }
    }
    append_log(paths, &format!("server generation {generation} stopped"));
    Ok(())
}

fn wait_at_test_startup_gate() -> Result<()> {
    let Some(path) = std::env::var_os("BRAIN_TEST_SERVER_STARTUP_GATE") else {
        return Ok(());
    };
    let mut gate =
        UnixStream::connect(PathBuf::from(path)).context("connecting startup test gate")?;
    gate.write_all(b"ready")
        .context("signaling startup test gate")?;
    let mut release = [0];
    gate.read_exact(&mut release)
        .context("waiting for startup test gate")
}

struct ProcessOwner {
    paths: ServerPaths,
    generation: ServerGeneration,
    _election: ElectionGuard,
}

impl Drop for ProcessOwner {
    fn drop(&mut self) {
        if matches!(
            super::state::remove_generation(&self.paths, self.generation),
            Ok(false)
        ) && super::validate_election_token(&self.paths, self.generation).is_ok()
        {
            let _ = super::state::remove_unpublished(&self.paths);
        }
    }
}

fn bind_control(paths: &ServerPaths) -> Result<UnixListener> {
    let listener = UnixListener::bind(paths.control_socket())
        .with_context(|| format!("binding {}", paths.control_socket().display()))?;
    fs::set_permissions(paths.control_socket(), fs::Permissions::from_mode(0o600))
        .with_context(|| format!("securing {}", paths.control_socket().display()))?;
    listener
        .set_nonblocking(true)
        .context("making server control socket nonblocking")?;
    Ok(listener)
}

fn termination_flag() -> Result<Arc<AtomicBool>> {
    use signal_hook::consts::signal::{SIGINT, SIGTERM};

    let flag = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(SIGINT, Arc::clone(&flag))?;
    signal_hook::flag::register(SIGTERM, Arc::clone(&flag))?;
    Ok(flag)
}

fn drain_control(
    listener: &UnixListener,
    generation: ServerGeneration,
    leases: &mut LeaseTable,
) -> Result<ServerDecision> {
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let decision = handle_control(&mut stream, generation, leases);
                if decision == ServerDecision::ShutdownNow {
                    return Ok(decision);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                return Ok(ServerDecision::KeepRunning);
            }
            Err(error) => return Err(error).context("accepting server control request"),
        }
    }
}

fn handle_control(
    stream: &mut UnixStream,
    generation: ServerGeneration,
    leases: &mut LeaseTable,
) -> ServerDecision {
    let _ = stream.set_read_timeout(Some(STREAM_TIMEOUT));
    let _ = stream.set_write_timeout(Some(STREAM_TIMEOUT));
    let result = serde_json::from_reader::<_, ControlRequest>(&mut *stream)
        .context("parsing server control request")
        .and_then(|request| apply_control(request, leases));
    let (response, decision) = match result {
        Ok(decision) => (ControlResponse::ok(generation, decision), decision),
        Err(error) => (
            ControlResponse::error(generation, error.to_string()),
            ServerDecision::KeepRunning,
        ),
    };
    let _ = serde_json::to_writer(&mut *stream, &response);
    let _ = stream.flush();
    decision
}

fn apply_control(request: ControlRequest, leases: &mut LeaseTable) -> Result<ServerDecision> {
    let now = Instant::now();
    match request {
        ControlRequest::Ping => Ok(ServerDecision::KeepRunning),
        ControlRequest::Register {
            lease_id,
            workspace_id,
            canonical_name,
            ingress_id,
            tui_pid,
            job_socket,
            receiver_enabled,
        } => {
            let expires_at = now
                .checked_add(super::LEASE_TTL)
                .context("lease expiry exceeds the monotonic clock range")?;
            let lease = WorkspaceLease {
                lease_id,
                workspace_id,
                canonical_name: WorkspaceName::parse(&canonical_name)?,
                ingress_id,
                tui_pid,
                job_socket,
                receiver_enabled,
                expires_at,
            };
            leases
                .apply(LeaseAction::Register { lease, now })
                .map_err(Into::into)
        }
        ControlRequest::Unregister { lease_id } => leases
            .apply(LeaseAction::Unregister { lease_id, now })
            .map_err(Into::into),
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum ControlRequest {
    Ping,
    Register {
        lease_id: LeaseId,
        workspace_id: WorkspaceId,
        canonical_name: String,
        ingress_id: IngressId,
        tui_pid: u32,
        job_socket: PathBuf,
        receiver_enabled: bool,
    },
    Unregister {
        lease_id: LeaseId,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ControlResponse {
    generation: ServerGeneration,
    shutdown: bool,
    error: Option<String>,
}

impl ControlResponse {
    const fn ok(generation: ServerGeneration, decision: ServerDecision) -> Self {
        Self {
            generation,
            shutdown: matches!(decision, ServerDecision::ShutdownNow),
            error: None,
        }
    }

    fn error(generation: ServerGeneration, error: String) -> Self {
        Self {
            generation,
            shutdown: false,
            error: Some(error),
        }
    }

    const fn decision(&self) -> ServerDecision {
        if self.shutdown {
            ServerDecision::ShutdownNow
        } else {
            ServerDecision::KeepRunning
        }
    }
}

fn append_log(paths: &ServerPaths, message: &str) {
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths.log())
    {
        let _ = writeln!(file, "{} {message}", chrono::Utc::now().to_rfc3339());
    }
}
