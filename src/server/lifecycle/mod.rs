//! Election, generation state, leases, and automatic lifetime of the one
//! machine-wide Brain server process.

mod decision;
mod election;
mod lease;
mod paths;
mod process;
mod state;
mod table;
mod watchdog;

pub use crate::server::control::ServerClient;
pub(crate) use decision::AuthorityRevision;
pub use decision::ServerDecision;
pub use election::{
    ElectionGuard, ElectionHandoff, StartDecision, decide_start, validate_election_token,
};
pub use lease::{
    HEARTBEAT_INTERVAL, IngressId, IngressIdError, LEASE_TTL, LeaseId, LeaseIdError, LeaseTiming,
    WorkspaceAvailability, WorkspaceLease,
};
pub use paths::ServerPaths;
pub(crate) use process::connect_or_elect_until;
pub use process::{choose_port, connect_or_elect, logs, run_process, status};
pub(crate) use state::read_record;
pub use state::{ProcessRecord, ServerGeneration, ServerGenerationError};
pub(crate) use table::LeaseStatusView;
pub use table::{LeaseAction, LeaseError, LeaseTable};

/// True if a process with `pid` exists. This stable path is also used by sync.
#[must_use]
pub fn pid_alive(pid: u32) -> bool {
    CommandPidProbe::alive(pid)
}

struct CommandPidProbe;

impl CommandPidProbe {
    fn alive(pid: u32) -> bool {
        std::process::Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }
}
