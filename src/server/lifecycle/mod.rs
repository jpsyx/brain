//! Lifecycle of the background brain server: the on-disk daemon record, the
//! pure liveness / port decisions, the thin IO probes around them, and the
//! `start` / `status` / `kill` CLI actions.
//!
//! There is **one** brain server per machine, shared across every `brain`
//! invocation and tab. Its record lives at `~/.cache/brain/server.json`
//! (`{pid, port}`); [`running`] reads it and confirms the process is actually
//! alive and the port reachable, reaping a stale record otherwise, and
//! [`ensure_running`] reuses a live one or spawns a fresh detached daemon.

use std::io::Write;
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::theme::Theme;

mod decision;
mod lease;
mod table;

pub use decision::ServerDecision;
pub use lease::{
    HEARTBEAT_INTERVAL, IngressId, IngressIdError, LEASE_TTL, LeaseId, LeaseIdError, LeaseTiming,
    WorkspaceAvailability, WorkspaceLease,
};
pub use table::{LeaseAction, LeaseError, LeaseTable};

/// The port the daemon prefers; if it is free the server binds it, otherwise
/// the OS assigns an ephemeral one.
const PREFERRED_PORT: u16 = 8787;

/// How long [`ensure_running`] waits for a freshly spawned daemon to publish
/// its state before giving up.
const SPAWN_WAIT: Duration = Duration::from_secs(2);

/// How long [`port_reachable`] waits for a TCP connect before deciding the
/// port is dead.
const REACHABLE_TIMEOUT: Duration = Duration::from_millis(300);

/// The persisted record of the running brain server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerState {
    pub pid: u32,
    pub port: u16,
}

/// Where the daemon record lives: `~/.cache/brain/server.json`. Mirrors
/// [`crate::state::Db::default_path`].
#[must_use]
pub fn state_path() -> PathBuf {
    let base = std::env::var_os("HOME").map_or_else(
        || PathBuf::from("."),
        |h| PathBuf::from(h).join(".cache").join("brain"),
    );
    base.join("server.json")
}

/// Whether the recorded server is actually live: a record exists, its process
/// is alive, and its port answers. Pure; the IO probes are the caller's job.
#[must_use]
pub fn is_live(state: Option<&ServerState>, pid_alive: bool, port_reachable: bool) -> bool {
    state.is_some() && pid_alive && port_reachable
}

/// Pick the port to hand the daemon: the preferred port when it is free, else
/// `0` (let the OS assign an ephemeral port). Pure.
#[must_use]
pub fn choose_port(preferred_free: bool, preferred: u16) -> u16 {
    if preferred_free { preferred } else { 0 }
}

// -- thin IO around the pure decisions -----------------------------------

/// Read and parse the daemon record; `None` when it is absent or unparseable.
#[must_use]
pub fn read_state() -> Option<ServerState> {
    crate::logging::log(format!("server read state {}", state_path().display()));
    let raw = std::fs::read_to_string(state_path()).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Write the daemon record, creating `~/.cache/brain/` as needed.
///
/// # Errors
/// Returns an error if the cache directory can't be created or the file can't
/// be written.
pub fn write_state(state: ServerState) -> Result<()> {
    let path = state_path();
    crate::logging::log(format!(
        "server write state path={} pid={} port={}",
        path.display(),
        state.pid,
        state.port
    ));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let json = serde_json::to_string(&state).context("serializing server state")?;
    std::fs::write(&path, json).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// Delete the daemon record, if present. Best-effort.
pub fn remove_state() {
    crate::logging::log(format!("server remove state {}", state_path().display()));
    let _ = std::fs::remove_file(state_path());
}

/// True if a process with `pid` exists. Uses `kill -0` (sends no signal), so
/// it stays dependency- and `unsafe`-free (mirrors [`crate::state::system_pid_alive`]).
#[must_use]
pub fn pid_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// True if something answers on `127.0.0.1:port` within [`REACHABLE_TIMEOUT`].
#[must_use]
pub fn port_reachable(port: u16) -> bool {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    TcpStream::connect_timeout(&addr, REACHABLE_TIMEOUT).is_ok()
}

/// True if the preferred port is currently bindable (probe, then release).
fn preferred_is_free(preferred: u16) -> bool {
    TcpListener::bind((Ipv4Addr::LOCALHOST, preferred)).is_ok()
}

/// The port of the live brain server, or `None`. Reaps a stale record.
#[must_use]
pub fn running() -> Option<u16> {
    let state = read_state()?;
    crate::logging::log(format!(
        "server state found pid={} port={}",
        state.pid, state.port
    ));
    if is_live(
        Some(&state),
        pid_alive(state.pid),
        port_reachable(state.port),
    ) {
        crate::logging::log(format!("server live port={}", state.port));
        Some(state.port)
    } else {
        crate::logging::log("server state stale");
        remove_state();
        None
    }
}

/// Ensure the shared brain server is up, returning its port. Reuses a live
/// daemon; otherwise spawns a fresh detached one and waits for it to publish
/// its state.
///
/// # Errors
/// Returns an error if the daemon can't be spawned or fails to come up within
/// [`SPAWN_WAIT`].
pub fn ensure_running() -> Result<u16> {
    if let Some(port) = running() {
        crate::logging::log(format!("server reuse port={port}"));
        return Ok(port);
    }
    let port = choose_port(preferred_is_free(PREFERRED_PORT), PREFERRED_PORT);
    crate::logging::log(format!("server spawn requested_port={port}"));
    spawn_daemon(port).context("spawning the brain server daemon")?;

    let deadline = Instant::now() + SPAWN_WAIT;
    loop {
        if let Some(port) = running() {
            crate::logging::log(format!("server spawned port={port}"));
            return Ok(port);
        }
        if Instant::now() >= deadline {
            crate::logging::log("server spawn timed out");
            anyhow::bail!("brain server did not come up within {SPAWN_WAIT:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[must_use]
pub fn format_ensure_plan(theme: Theme) -> String {
    format!(
        "{}\n  {} {}\n  {} {}",
        theme.heading("Checking the brain server"),
        theme.muted("state:"),
        "~/.cache/brain/server.json",
        theme.muted("plan:"),
        "reuse a live daemon, or start one if needed",
    )
}

/// Spawn `brain server run --port <port>` detached (own process group, null
/// stdio) so it outlives this process without a controlling terminal. No
/// `unsafe`: detachment is `process_group(0)` plus null stdio.
fn spawn_daemon(port: u16) -> Result<()> {
    use std::os::unix::process::CommandExt;
    let exe = std::env::current_exe().context("resolving the current executable")?;
    crate::logging::log(format!(
        "spawn brain server exe={} port={port}",
        exe.display()
    ));
    std::process::Command::new(exe)
        .args(["server", "run", "--port", &port.to_string()])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn()
        .context("spawning detached brain server")?;
    Ok(())
}

// -- CLI actions ---------------------------------------------------------

/// `brain server start`: ensure the daemon is up and report where.
///
/// # Errors
/// Propagates a spawn/startup failure from [`ensure_running`].
pub fn start() -> Result<()> {
    let theme = Theme::active();
    eprintln!("{}", format_ensure_plan(theme));
    crate::logging::log("server start ensure_running");
    let port = ensure_running()?;
    let mut out = std::io::stdout();
    writeln!(out, "{}", theme.success(&running_line(port)))?;
    Ok(())
}

/// `brain server status`: report whether the daemon is running and where.
///
/// # Errors
/// Propagates a failure writing to stdout.
pub fn status() -> Result<()> {
    let theme = Theme::active();
    let mut out = std::io::stdout();
    match running() {
        Some(port) => writeln!(out, "{}", theme.success(&running_line(port)))?,
        None => writeln!(out, "{}", theme.muted("brain server is not running"))?,
    }
    Ok(())
}

/// `brain server kill`: stop the daemon (SIGTERM) and drop its record.
///
/// # Errors
/// Propagates a failure writing to stdout.
pub fn kill() -> Result<()> {
    let theme = Theme::active();
    let mut out = std::io::stdout();
    match read_state() {
        Some(state) => {
            crate::logging::log(format!("server kill pid={}", state.pid));
            let _ = std::process::Command::new("kill")
                .arg(state.pid.to_string())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            remove_state();
            writeln!(
                out,
                "{}",
                theme.warning(&format!("stopped brain server (pid {})", state.pid))
            )?;
        }
        None => writeln!(out, "{}", theme.muted("brain server is not running"))?,
    }
    Ok(())
}

fn running_line(port: u16) -> String {
    format!("\u{2713} brain server running on http://127.0.0.1:{port}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_live_is_false_with_no_state() {
        assert!(!is_live(None, true, true));
    }

    #[test]
    fn is_live_is_true_when_present_alive_and_reachable() {
        let s = ServerState { pid: 1, port: 8787 };
        assert!(is_live(Some(&s), true, true));
    }

    #[test]
    fn is_live_is_false_when_alive_but_unreachable() {
        let s = ServerState { pid: 1, port: 8787 };
        assert!(!is_live(Some(&s), true, false));
    }

    #[test]
    fn is_live_is_false_when_dead() {
        let s = ServerState { pid: 1, port: 8787 };
        assert!(!is_live(Some(&s), false, true));
    }

    #[test]
    fn choose_port_uses_preferred_when_free() {
        assert_eq!(choose_port(true, 8787), 8787);
    }

    #[test]
    fn choose_port_falls_back_to_zero_when_busy() {
        assert_eq!(choose_port(false, 8787), 0);
    }

    #[test]
    fn ensure_plan_says_server_state_and_daemon_spawn_may_happen() {
        let plan = format_ensure_plan(Theme::dark(false));

        assert!(plan.contains("Checking the brain server"), "{plan}");
        assert!(plan.contains("state: ~/.cache/brain/server.json"), "{plan}");
        assert!(plan.contains("reuse a live daemon"), "{plan}");
        assert!(plan.contains("start one if needed"), "{plan}");
    }
}

#[cfg(test)]
mod lease_table_tests {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use super::{
        IngressId, LeaseError, LeaseId, LeaseTable, LeaseTiming, WorkspaceAvailability,
        WorkspaceLease,
    };
    use crate::workspace::{WorkspaceId, WorkspaceName};

    const FAMILY_ID: &str = "e806258e-491a-436d-9db4-a5ca9903e0d4";
    const PERSONAL_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";
    const FAMILY_INGRESS: &str = "57b162df-983a-45c3-ac7e-bad94eb27a99";
    const PERSONAL_INGRESS: &str = "91a0cfc2-7427-49d5-a2f1-258f985cd7e5";

    #[test]
    fn leases_for_different_workspaces_coexist_and_final_removal_shuts_down() {
        let now = Instant::now();
        let mut table = LeaseTable::default();
        let family = lease("family", FAMILY_ID, FAMILY_INGRESS, lease_id(1), now, true);
        let personal = lease(
            "personal",
            PERSONAL_ID,
            PERSONAL_INGRESS,
            lease_id(2),
            now,
            true,
        );

        table.register(family, now).expect("family lease registers");
        table
            .register(personal, now)
            .expect("personal lease registers");

        assert_eq!(table.live_workspaces(now), [family_id(), personal_id()]);
        assert_eq!(
            table.unregister(lease_id(1), now),
            super::ServerDecision::KeepRunning
        );
        assert_eq!(
            table.unregister(lease_id(2), now),
            super::ServerDecision::ShutdownNow
        );
    }

    #[test]
    fn rejects_a_second_live_lease_for_one_workspace() {
        let now = Instant::now();
        let mut table = LeaseTable::default();
        table
            .register(
                lease("family", FAMILY_ID, FAMILY_INGRESS, lease_id(1), now, true),
                now,
            )
            .expect("first lease registers");
        let result = table.register(
            lease("family", FAMILY_ID, FAMILY_INGRESS, lease_id(2), now, true),
            now,
        );

        assert!(matches!(
            result,
            Err(LeaseError::WorkspaceAlreadyLeased { .. })
        ));
    }

    #[test]
    fn heartbeat_renews_only_its_matching_live_lease() {
        let now = Instant::now();
        let timing = LeaseTiming::new(Duration::from_millis(5), Duration::from_secs(10));
        let mut table = LeaseTable::default();
        table
            .register(
                lease("family", FAMILY_ID, FAMILY_INGRESS, lease_id(1), now, true),
                now,
            )
            .expect("family lease registers");
        table
            .register(
                lease(
                    "personal",
                    PERSONAL_ID,
                    PERSONAL_INGRESS,
                    lease_id(2),
                    now,
                    true,
                ),
                now,
            )
            .expect("personal lease registers");

        table
            .heartbeat(lease_id(1), now + Duration::from_secs(1), timing)
            .expect("matching lease renews");

        assert!(matches!(
            table.availability(ingress(FAMILY_INGRESS), now + Duration::from_secs(10)),
            WorkspaceAvailability::Accepting(_)
        ));
        assert_eq!(
            table.availability(ingress(PERSONAL_INGRESS), now + Duration::from_secs(10)),
            WorkspaceAvailability::NoLiveTui
        );
    }

    #[test]
    fn expiry_removes_the_lease_without_returning_stale_routing_data() {
        let now = Instant::now();
        let mut table = LeaseTable::default();
        table
            .register(
                lease("family", FAMILY_ID, FAMILY_INGRESS, lease_id(1), now, true),
                now,
            )
            .expect("lease registers");

        assert_eq!(
            table.expire(now + Duration::from_secs(10)),
            super::ServerDecision::ShutdownNow
        );
        assert_eq!(
            table.availability(ingress(FAMILY_INGRESS), now + Duration::from_secs(10)),
            WorkspaceAvailability::NoLiveTui
        );
    }

    #[test]
    fn stale_same_workspace_lease_is_replaced_after_server_recovery() {
        let now = Instant::now();
        let mut table = LeaseTable::default();
        table
            .register(
                lease("family", FAMILY_ID, FAMILY_INGRESS, lease_id(1), now, true),
                now,
            )
            .expect("first lease registers");
        table
            .register(
                lease(
                    "personal",
                    PERSONAL_ID,
                    PERSONAL_INGRESS,
                    lease_id(3),
                    now + Duration::from_secs(7),
                    true,
                ),
                now,
            )
            .expect("unrelated lease registers");

        table
            .register(
                lease(
                    "family",
                    FAMILY_ID,
                    FAMILY_INGRESS,
                    lease_id(2),
                    now + Duration::from_secs(10),
                    true,
                ),
                now + Duration::from_secs(10),
            )
            .expect("stale lease is replaced");

        assert!(matches!(
            table.availability(ingress(FAMILY_INGRESS), now + Duration::from_secs(10)),
            WorkspaceAvailability::Accepting(WorkspaceLease { lease_id: found, .. }) if found == lease_id(2)
        ));
        assert!(matches!(
            table.availability(ingress(PERSONAL_INGRESS), now + Duration::from_secs(10)),
            WorkspaceAvailability::Accepting(WorkspaceLease { lease_id: found, .. }) if found == lease_id(3)
        ));
    }

    #[test]
    fn rejects_an_ingress_collision_between_live_workspaces() {
        let now = Instant::now();
        let mut table = LeaseTable::default();
        table
            .register(
                lease("family", FAMILY_ID, FAMILY_INGRESS, lease_id(1), now, true),
                now,
            )
            .expect("family lease registers");

        let result = table.register(
            lease(
                "personal",
                PERSONAL_ID,
                FAMILY_INGRESS,
                lease_id(2),
                now,
                true,
            ),
            now,
        );

        assert!(matches!(
            result,
            Err(LeaseError::IngressAlreadyLeased { .. })
        ));
    }

    #[test]
    fn disabled_live_lease_is_distinct_from_no_live_tui_and_unknown_ingress() {
        let now = Instant::now();
        let mut table = LeaseTable::default();
        table
            .register(
                lease("family", FAMILY_ID, FAMILY_INGRESS, lease_id(1), now, false),
                now,
            )
            .expect("disabled lease registers");

        assert_eq!(
            table.availability(ingress(FAMILY_INGRESS), now),
            WorkspaceAvailability::Disabled
        );
        assert_eq!(
            table.unregister(lease_id(1), now),
            super::ServerDecision::ShutdownNow
        );
        assert_eq!(
            table.availability(ingress(FAMILY_INGRESS), now),
            WorkspaceAvailability::NoLiveTui
        );
        assert_eq!(
            table.availability(ingress(PERSONAL_INGRESS), now),
            WorkspaceAvailability::Unknown
        );
    }

    fn lease(
        canonical_name: &str,
        workspace_id: &str,
        ingress_id: &str,
        lease_id: LeaseId,
        now: Instant,
        receiver_enabled: bool,
    ) -> WorkspaceLease {
        WorkspaceLease {
            lease_id,
            workspace_id: WorkspaceId::parse(workspace_id).expect("valid workspace UUID"),
            canonical_name: WorkspaceName::parse(canonical_name).expect("valid workspace name"),
            ingress_id: ingress(ingress_id),
            tui_pid: 42,
            job_socket: PathBuf::from("/tmp/brain-job.sock"),
            receiver_enabled,
            expires_at: now + Duration::from_secs(5),
        }
    }

    fn family_id() -> WorkspaceId {
        WorkspaceId::parse(FAMILY_ID).expect("valid family UUID")
    }

    fn personal_id() -> WorkspaceId {
        WorkspaceId::parse(PERSONAL_ID).expect("valid personal UUID")
    }

    fn ingress(value: &str) -> IngressId {
        IngressId::parse(value).expect("valid ingress UUID")
    }

    fn lease_id(last: u128) -> LeaseId {
        LeaseId::parse(&format!("00000000-0000-0000-0000-{last:012x}")).expect("valid lease UUID")
    }
}
