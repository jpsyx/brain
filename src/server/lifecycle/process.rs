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
    match ServerClient::default().snapshot() {
        Ok((record, snapshot)) => {
            writeln!(output, "{}", theme.success("Brain server  running"))?;
            writeln!(
                output,
                "{}  {}",
                theme.muted("Process"),
                theme.value(&record.pid.to_string())
            )?;
            writeln!(
                output,
                "{}       {}",
                theme.muted("Live TUIs"),
                theme.value(&snapshot.live_leases.to_string())
            )?;
        }
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
    connect_or_elect_until_with_mode(client, deadline, false)
}

pub fn connect_or_elect_background(client: &ServerClient) -> Result<ProcessRecord> {
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    connect_or_elect_until_with_mode(client, deadline, true)
}

fn connect_or_elect_until_with_mode(
    client: &ServerClient,
    deadline: Instant,
    background: bool,
) -> Result<ProcessRecord> {
    connect_or_elect_until_with_publication_hook_and_mode(client, deadline, &mut |_| {}, background)
}

pub(crate) fn connect_or_elect_until_with_publication_hook(
    client: &ServerClient,
    deadline: Instant,
    after_publication: &mut impl FnMut(&ProcessRecord),
) -> Result<ProcessRecord> {
    connect_or_elect_until_with_publication_hook_and_mode(
        client,
        deadline,
        after_publication,
        false,
    )
}

fn connect_or_elect_until_with_publication_hook_and_mode(
    client: &ServerClient,
    deadline: Instant,
    after_publication: &mut impl FnMut(&ProcessRecord),
    background: bool,
) -> Result<ProcessRecord> {
    let mut legacy_protocol_observed = false;
    loop {
        let record = super::state::read_record(client.paths());
        let process_live = record.as_ref().is_some_and(|state| pid_alive(state.pid));
        let socket_live =
            record
                .as_ref()
                .is_some_and(|state| match client.connect_existing_until(deadline) {
                    Ok(found) => found == *state,
                    Err(error) => {
                        legacy_protocol_observed |=
                            crate::server::control::is_protocol_mismatch(&error);
                        false
                    }
                });
        if process_live && socket_live {
            return Ok(record.expect("live probes require a process record"));
        }
        if process_live && legacy_protocol_observed {
            if Instant::now() >= deadline {
                return Err(anyhow::Error::new(crate::server::control::ProtocolMismatch));
            }
            std::thread::park_timeout(POLL_INTERVAL);
            continue;
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
                let mut starter = client.spawn(guard.generation(), port, background)?;
                let handoff = guard.handoff();
                let connection = wait_for_started_connection(client, deadline, &mut starter);
                match connection {
                    Ok(Some(found)) => {
                        reap_published_child(starter);
                        after_publication(&found);
                        handoff.cleanup()?;
                        return Ok(found);
                    }
                    Ok(None) => handoff.cleanup()?,
                    Err(error) => {
                        handoff.cleanup()?;
                        return Err(error);
                    }
                }
            }
            StartDecision::WaitForWinner => {
                if let Some(found) = wait_for_winner(client, deadline)? {
                    return Ok(found);
                }
            }
        }
        if Instant::now() >= deadline {
            if legacy_protocol_observed {
                return Err(anyhow::Error::new(crate::server::control::ProtocolMismatch));
            }
            anyhow::bail!("brain server did not come up within {STARTUP_TIMEOUT:?}");
        }
    }
}

fn reap_published_child(mut child: std::process::Child) {
    std::thread::Builder::new()
        .name("brain-server-reaper".to_owned())
        .spawn(move || {
            if let Err(error) = child.wait() {
                crate::logging::log(format!("shared-server child reap failed: {error}"));
            }
        })
        .expect("shared-server child reaper thread must start");
}

fn wait_for_winner(client: &ServerClient, deadline: Instant) -> Result<Option<ProcessRecord>> {
    let observation_deadline = deadline.min(Instant::now() + Duration::from_millis(100));
    loop {
        if let Ok(record) = client.connect_existing_until(observation_deadline) {
            return Ok(Some(record));
        }
        if retry_winner_election(
            client.paths().election_lock().exists(),
            Instant::now(),
            observation_deadline,
        ) {
            return Ok(None);
        }
        std::thread::park_timeout(POLL_INTERVAL);
    }
}

fn wait_for_started_connection(
    client: &ServerClient,
    deadline: Instant,
    starter: &mut std::process::Child,
) -> Result<Option<ProcessRecord>> {
    loop {
        let observation_deadline = deadline.min(Instant::now() + Duration::from_millis(100));
        if let Ok(record) = client.connect_existing_until(observation_deadline) {
            return Ok(Some(record));
        }
        let starter_exited = starter
            .try_wait()
            .context("observing elected shared-server starter")?
            .is_some();
        if retry_after_starter_wait(starter_exited, Instant::now(), deadline) {
            return Ok(None);
        }
        std::thread::park_timeout(POLL_INTERVAL);
    }
}

fn retry_winner_election(
    election_token_exists: bool,
    now: Instant,
    observation_deadline: Instant,
) -> bool {
    !election_token_exists || now >= observation_deadline
}

fn retry_after_starter_wait(starter_exited: bool, now: Instant, deadline: Instant) -> bool {
    starter_exited || now >= deadline
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
pub fn run_process(
    paths: &ServerPaths,
    generation: ServerGeneration,
    port: u16,
    background: bool,
) -> Result<()> {
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
    let watchdog = if background {
        Watchdog::background(Instant::now())
    } else {
        Watchdog::new(Instant::now(), INITIAL_REGISTRATION_TIMEOUT)
    };

    while !terminate.load(Ordering::Relaxed) {
        let decision = {
            let control_decision = control.drain(&control_server)?;
            if control_decision == ServerDecision::ShutdownNow {
                control_decision
            } else {
                let now = Instant::now();
                let deadline = now + Duration::from_secs(2);
                let expiry_decision = ControlServer::expire_shared_until(
                    &control_server,
                    now,
                    deadline,
                    &Instant::now,
                )?;
                if expiry_decision == ServerDecision::ShutdownNow {
                    expiry_decision
                } else {
                    let mut control_server = control_server
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    watchdog.tick(control_server.leases_mut(), now)?
                }
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

/// Append one already-redacted event to the stream viewed by `brain server logs`.
#[cfg(not(test))]
pub(crate) fn append_event_log(message: &str) {
    append_event_log_to(&ServerPaths::default(), message);
}

fn append_event_log_to(paths: &ServerPaths, message: &str) {
    append_log(paths, message);
}

#[cfg(test)]
mod retry_tests;

#[cfg(test)]
mod event_log_tests {
    use super::*;

    #[test]
    fn receiver_events_append_to_the_exact_stream_server_logs_reads() {
        let temporary = tempfile::tempdir().expect("server log directory");
        let paths = ServerPaths::from_directory(temporary.path().to_owned());
        let event = "receiver lifecycle event=claim phase=claimed queue_depth=2";

        append_event_log_to(&paths, event);

        let contents = fs::read_to_string(paths.log()).expect("read server log stream");
        assert!(contents.contains(event), "{contents}");
        assert!(!contents.contains("private-workspace"));
        assert!(!contents.contains("private-prompt"));
    }
}
