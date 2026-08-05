//! Pure lifecycle outcomes for the shared server.

/// Whether a lease transition leaves the shared server needed by a live TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerDecision {
    /// At least one live workspace lease remains, or no lease was removed.
    KeepRunning,
    /// The final live workspace lease was removed or expired.
    ShutdownNow,
}

/// Monotonic identity for one workspace lease authority incarnation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AuthorityRevision(u64);

impl AuthorityRevision {
    #[cfg(test)]
    pub(crate) const fn initial() -> Self {
        Self(1)
    }

    #[cfg(test)]
    pub(crate) const fn from_raw(value: u64) -> Self {
        Self(value)
    }
}

/// Whether a lease transition preserves or replaces routing authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthorityChange {
    Heartbeat,
    Revoked,
}

/// Decide the authority revision after one pure lifecycle transition.
pub(crate) const fn authority_revision_after(
    current: Option<AuthorityRevision>,
    change: AuthorityChange,
) -> Option<AuthorityRevision> {
    match (current, change) {
        (Some(revision), AuthorityChange::Heartbeat) => Some(revision),
        (None, AuthorityChange::Heartbeat) => None,
        (None, AuthorityChange::Revoked) => Some(AuthorityRevision(1)),
        (Some(AuthorityRevision(value)), AuthorityChange::Revoked) => match value.checked_add(1) {
            Some(next) => Some(AuthorityRevision(next)),
            None => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{AuthorityChange, AuthorityRevision, authority_revision_after};

    #[test]
    fn heartbeat_preserves_authority_but_revocation_advances_its_incarnation() {
        let original = AuthorityRevision::initial();

        assert_eq!(
            authority_revision_after(Some(original), AuthorityChange::Heartbeat),
            Some(original)
        );
        assert_eq!(
            authority_revision_after(Some(original), AuthorityChange::Revoked),
            Some(AuthorityRevision::from_raw(2))
        );
        assert_eq!(
            authority_revision_after(None, AuthorityChange::Revoked),
            Some(original)
        );
    }
}
