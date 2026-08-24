//! Persistent state shared across `brain` shells and the Claude
//! SessionStart hook.
//!
//! Backed by SQLite (WAL mode) so multiple `brain` shells and the hook
//! script (a separate process) can read/write the same DB without
//! clobbering or busy-waiting. Mirrors the `tasks` sibling project's state
//! layer, scoped to what brain needs.
//!
//! Four tables:
//! - `brain_sessions` stores frontend sessions with immutable workspace,
//!   actor, and channel attribution. `locked_pid` is the PID of the live brain shell currently
//!   driving that session (NULL when free). The session-resume model is
//!   "lock + recency": on startup we resume the most-recently-active free
//!   session and lock it; on exit we release the lock; stale locks (dead
//!   PIDs) are reaped on the next startup. This keeps two terminals off the
//!   same conversation thread while still resuming your latest work.
//! - `meta` is a small key/value store; today just the `panel_side` layout
//!   preference (which side the brain panel sits on).
//! - `receiver_conversations` stores one logical workspace/user/channel
//!   lineage with its portable transcript and current native session binding.
//! - `receiver_jobs` stores immutable accepted inputs, explicit lifecycle and
//!   retry state, plus expiring non-destructive claim ownership.
//!
//! The SessionStart hook requires the selected workspace/actor variables
//! plus `BRAIN_INSTANCE_ID` and the selected UUID-scoped `BRAIN_STATE_DB`.
//! `BRAIN_PID` supplies optional lock ownership. Incomplete attribution means
//! ambient Claude usage, so the hook no-ops.

use std::path::Path;

use crate::agent::CompletionStatus;
pub use crate::agent::SessionScope;
use anyhow::{Context, Result};
use rusqlite::Connection;

/// Wall-clock provider (unix seconds). Production uses [`system_clock`];
/// tests inject a deterministic value.
pub type Clock = fn() -> i64;

/// Process-liveness probe. Production uses [`system_pid_alive`]; tests
/// inject a deterministic predicate.
pub type PidAlive = fn(i32) -> bool;

#[must_use]
pub fn system_clock() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0))
}

/// True if a process with `pid` currently exists. Implemented via `kill -0`,
/// which sends no signal — it only checks existence — so it stays
/// dependency-free and `unsafe`-free.
#[must_use]
pub fn system_pid_alive(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Which side of the screen the brain (Claude) panel sits on. The fuzzy
/// search panel takes the other side. Persisted in `meta('panel_side')`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelSide {
    Left,
    Right,
}

impl PanelSide {
    /// Default: brain panel on the right, search on the left.
    pub const DEFAULT: Self = Self::Right;

    #[must_use]
    pub const fn flipped(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }

    #[must_use]
    const fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
        }
    }

    #[must_use]
    fn parse(s: &str) -> Self {
        match s {
            "left" => Self::Left,
            _ => Self::Right,
        }
    }
}

pub struct Db {
    conn: Connection,
    workspace_id: String,
    clock: Clock,
    pid_alive: PidAlive,
}

mod database;
mod receiver;
mod session_store;
pub(crate) use receiver::schema::down_path as receiver_schema_down;
pub(crate) use receiver::schema::down_to_previous_path as receiver_launch_schema_down;
pub use receiver::{
    EmailLineage, EmailLineageError, MAX_RECEIVER_LAUNCH_ATTEMPTS, ReceiverAcceptance,
    ReceiverClaim, ReceiverConversation, ReceiverConversationId, ReceiverConversationIdentity,
    ReceiverJob, ReceiverJobId, ReceiverJobState, ReceiverLaunchFailure,
    ReceiverLaunchRetryOutcome, ReceiverRunClaim, ReceiverSessionBinding,
    ReceiverSessionBindingError, ReceiverSessionPlan,
};
#[cfg(test)]
mod tests;
