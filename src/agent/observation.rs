//! Bounded frontend-neutral receiver lifecycle observations.

use std::{
    fmt::{Display, Formatter},
    path::PathBuf,
};

use super::{AgentKind, AgentSession};

const MAX_IDENTIFIER_BYTES: usize = 256;
const LAUNCHED_BIT: u8 = 1;
const ACCEPTED_BIT: u8 = 1 << 1;
const PROGRESSING_BIT: u8 = 1 << 2;
const COMPLETED_BIT: u8 = 1 << 3;

mod snapshot;
pub(crate) use snapshot::read_normalized_snapshot;

/// Opaque progress marker returned by one successful observation poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentObservationCursor {
    revision: i64,
    represented: u8,
    accepted_at_unix_ms: Option<u64>,
    progressing_at_unix_ms: Option<u64>,
    completed_at_unix_ms: Option<u64>,
}

impl AgentObservationCursor {
    /// Begin after Brain's already-durable launched boundary.
    #[must_use]
    pub const fn launched() -> Self {
        Self {
            revision: 0,
            represented: LAUNCHED_BIT,
            accepted_at_unix_ms: None,
            progressing_at_unix_ms: None,
            completed_at_unix_ms: None,
        }
    }

    #[cfg(test)]
    pub(crate) const fn at_revision(
        revision: i64,
        accepted_at_unix_ms: Option<u64>,
        progressing_at_unix_ms: Option<u64>,
        completed_at_unix_ms: Option<u64>,
    ) -> Self {
        Self {
            revision,
            represented: LAUNCHED_BIT | ACCEPTED_BIT | PROGRESSING_BIT | COMPLETED_BIT,
            accepted_at_unix_ms,
            progressing_at_unix_ms,
            completed_at_unix_ms,
        }
    }
}

/// Lifecycle semantics exposed outside frontend adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentObservationPhase {
    /// The child was launched and durably correlated.
    Launched,
    /// The frontend accepted the exact receiver prompt.
    Accepted,
    /// The frontend produced tool-backed progress.
    Progressing,
    /// The frontend completed the receiver turn.
    Completed,
}

/// One newly observed lifecycle boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentObservationBoundary {
    phase: AgentObservationPhase,
    observed_at_unix_ms: u64,
}

impl AgentObservationBoundary {
    #[cfg(test)]
    pub(crate) const fn new(phase: AgentObservationPhase, observed_at_unix_ms: u64) -> Self {
        Self {
            phase,
            observed_at_unix_ms,
        }
    }

    /// Neutral lifecycle boundary represented by this fact.
    #[must_use]
    pub const fn phase(self) -> AgentObservationPhase {
        self.phase
    }

    /// Producer timestamp retained from the normalized snapshot.
    #[must_use]
    pub const fn observed_at_unix_ms(self) -> u64 {
        self.observed_at_unix_ms
    }
}

/// Trusted identity and cursor for one observation poll.
#[derive(Clone)]
pub struct AgentObservationRequest {
    job_token: String,
    remote_instance: String,
    snapshot_path: PathBuf,
    lifecycle_session: AgentSession,
    cursor: AgentObservationCursor,
}

impl AgentObservationRequest {
    /// Bind one poll to its trusted launch identity.
    #[must_use]
    pub fn new(
        job_token: impl Into<String>,
        remote_instance: impl Into<String>,
        snapshot_path: PathBuf,
        lifecycle_session: AgentSession,
        cursor: AgentObservationCursor,
    ) -> Self {
        Self {
            job_token: job_token.into(),
            remote_instance: remote_instance.into(),
            snapshot_path,
            lifecycle_session,
            cursor,
        }
    }
}

impl std::fmt::Debug for AgentObservationRequest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AgentObservationRequest(<redacted>)")
    }
}

/// Content-free observations from one bounded poll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentObservationResult {
    session: AgentSession,
    boundaries: Vec<AgentObservationBoundary>,
    next_cursor: AgentObservationCursor,
}

impl AgentObservationResult {
    #[cfg(test)]
    pub(crate) fn new(
        session: AgentSession,
        boundaries: Vec<AgentObservationBoundary>,
        next_cursor: AgentObservationCursor,
    ) -> Self {
        Self {
            session,
            boundaries,
            next_cursor,
        }
    }

    /// Exact lifecycle-reported native session verified for this poll.
    #[must_use]
    pub const fn session(&self) -> &AgentSession {
        &self.session
    }

    /// Newly represented boundaries in lifecycle order.
    #[must_use]
    pub fn boundaries(&self) -> &[AgentObservationBoundary] {
        &self.boundaries
    }

    /// Cursor to supply to the next poll.
    #[must_use]
    pub const fn next_cursor(&self) -> AgentObservationCursor {
        self.next_cursor
    }
}

/// Stable conservative observation failures with no private payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentObservationError {
    /// A token or instance identifier is not a canonical UUID.
    InvalidIdentifier,
    /// The request did not name the canonical snapshot for this launch.
    WrongPath,
    /// The lifecycle session is a launch placeholder.
    PlaceholderSession,
    /// Current lifecycle ownership could not be verified.
    OwnershipUnavailable,
    /// The lifecycle session is not currently owned by the exact instance.
    SessionOwnership,
    /// The path is a symlink or not a regular file.
    InvalidFileType,
    /// The snapshot is accessible outside its owner.
    InvalidPermissions,
    /// The snapshot exceeds 4096 bytes.
    SnapshotTooLarge,
    /// A single bounded read could not produce the complete snapshot.
    TruncatedSnapshot,
    /// The snapshot is not valid schema-v1 JSON.
    MalformedSnapshot,
    /// Snapshot token or instance identity does not match the request.
    IdentityMismatch,
    /// Snapshot session identity does not match the lifecycle session.
    SessionMismatch,
    /// The snapshot revision moved backward.
    RevisionRegression,
    /// Lifecycle phase and timestamp fields are inconsistent.
    AmbiguousLifecycle,
}

impl Display for AgentObservationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidIdentifier => "agent observation identifier is invalid",
            Self::WrongPath => "agent observation path is invalid",
            Self::PlaceholderSession => "agent observation session is not established",
            Self::OwnershipUnavailable => "agent observation ownership is unavailable",
            Self::SessionOwnership => "agent observation session ownership is not current",
            Self::InvalidFileType => "agent observation file type is invalid",
            Self::InvalidPermissions => "agent observation permissions are invalid",
            Self::SnapshotTooLarge => "agent observation snapshot is too large",
            Self::TruncatedSnapshot => "agent observation snapshot is truncated",
            Self::MalformedSnapshot => "agent observation snapshot is malformed",
            Self::IdentityMismatch => "agent observation identity does not match",
            Self::SessionMismatch => "agent observation session does not match",
            Self::RevisionRegression => "agent observation revision regressed",
            Self::AmbiguousLifecycle => "agent observation lifecycle is ambiguous",
        })
    }
}

impl std::error::Error for AgentObservationError {}

pub(super) fn validate_controller_request(
    workspace: &crate::workspace::WorkspaceContext,
    actor: &crate::actor::ActorContext,
    kind: AgentKind,
    request: &AgentObservationRequest,
) -> Result<(), AgentObservationError> {
    if !is_canonical_uuid(&request.job_token)
        || !is_canonical_uuid(&request.remote_instance)
        || !valid_bounded_identifier(request.lifecycle_session.as_str())
    {
        return Err(AgentObservationError::InvalidIdentifier);
    }
    let expected = workspace
        .paths()
        .receiver_observations_dir()
        .join(format!("{}.json", request.remote_instance));
    if request.snapshot_path != expected {
        return Err(AgentObservationError::WrongPath);
    }
    if request
        .lifecycle_session
        .as_str()
        .starts_with("pending-receiver-")
    {
        return Err(AgentObservationError::PlaceholderSession);
    }
    validate_session_ownership(workspace, actor, kind, request)
}

pub(super) fn validate_session_ownership(
    workspace: &crate::workspace::WorkspaceContext,
    actor: &crate::actor::ActorContext,
    kind: AgentKind,
    request: &AgentObservationRequest,
) -> Result<(), AgentObservationError> {
    let connection = rusqlite::Connection::open_with_flags(
        workspace.paths().state_db(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| AgentObservationError::OwnershipUnavailable)?;
    let owned = connection
        .query_row(
            "SELECT EXISTS(
               SELECT 1 FROM brain_sessions
               WHERE brain_instance_id = ?1 AND agent_session_id = ?2
                 AND locked_pid IS NOT NULL AND agent_kind = ?3
                 AND workspace_id = ?4 AND actor_id = ?5 AND channel = ?6
             )",
            rusqlite::params![
                request.remote_instance,
                request.lifecycle_session.as_str(),
                kind.as_str(),
                workspace.id().to_string(),
                actor.user_id().as_str(),
                actor.channel().as_str(),
            ],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|_| AgentObservationError::OwnershipUnavailable)?;
    if !owned {
        return Err(AgentObservationError::SessionOwnership);
    }
    Ok(())
}

fn is_canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|parsed| parsed.hyphenated().to_string() == value)
}

fn valid_bounded_identifier(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_IDENTIFIER_BYTES && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod file_tests;
