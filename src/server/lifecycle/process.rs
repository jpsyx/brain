//! Thin process, control-socket, and client shells around lifecycle decisions.

use std::fs::{self, OpenOptions};
use std::io::{Read as _, Write as _};
use std::net::{Ipv4Addr, TcpListener};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::{
    ElectionGuard, ProcessRecord, ServerClient, ServerDecision, ServerGeneration, ServerPaths,
    StartDecision, decide_start, pid_alive, watchdog::Watchdog,
};
use crate::server::control::{ControlListener, ControlServer};
use crate::theme::Theme;
use anyhow::{Context, Result};

const PREFERRED_PORT: u16 = 8787;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(2);
const INITIAL_REGISTRATION_TIMEOUT: Duration = Duration::from_secs(2);
const POLL_INTERVAL: Duration = Duration::from_millis(25);
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
    connect_or_elect_until(client, deadline)
}

pub(crate) fn connect_or_elect_until(
    client: &ServerClient,
    deadline: Instant,
) -> Result<ProcessRecord> {
    loop {
        let record = super::state::read_record(client.paths());
        let process_live = record.as_ref().is_some_and(|state| pid_alive(state.pid));
        let socket_live = record.as_ref().is_some_and(|state| {
            client
                .connect_existing_until(deadline)
                .is_ok_and(|found| found == *state)
        });
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
                let connection = wait_for_connection(client, deadline);
                handoff.cleanup()?;
                if let Some(found) = connection? {
                    return Ok(found);
                }
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
        if let Ok(record) = client.connect_existing_until(deadline) {
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
    let control = ControlListener::bind(paths)?;
    let server = TcpListener::bind((Ipv4Addr::LOCALHOST, port))
        .with_context(|| format!("binding 127.0.0.1:{port}"))?;
    let actual_port = server
        .local_addr()
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
    let runtime_home = std::env::var_os("HOME").map_or_else(PathBuf::new, PathBuf::from);
    let control_server = Arc::new(Mutex::new(ControlServer::new(
        generation,
        crate::workspace::RegistryStore::real(),
        runtime_home,
    )));
    let http_workers = crate::server::http_workers::HttpWorkers::start(server, &control_server)?;
    let watchdog = Watchdog::new(Instant::now(), INITIAL_REGISTRATION_TIMEOUT);

    while !terminate.load(Ordering::Relaxed) {
        let decision = {
            let mut control_server = control_server
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let control_decision = control.drain(&mut control_server)?;
            if control_decision == ServerDecision::ShutdownNow {
                control_decision
            } else {
                watchdog.tick(control_server.leases_mut(), Instant::now())?
            }
        };
        if decision == ServerDecision::ShutdownNow {
            break;
        }
        std::thread::park_timeout(POLL_INTERVAL);
    }
    http_workers.finish_process_exit();
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

fn termination_flag() -> Result<Arc<AtomicBool>> {
    use signal_hook::consts::signal::{SIGINT, SIGTERM};

    let flag = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(SIGINT, Arc::clone(&flag))?;
    signal_hook::flag::register(SIGTERM, Arc::clone(&flag))?;
    Ok(flag)
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
