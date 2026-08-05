//! Pure lifecycle outcomes for the shared server.

/// Whether a lease transition leaves the shared server needed by a live TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerDecision {
    /// At least one live workspace lease remains, or no lease was removed.
    KeepRunning,
    /// The final live workspace lease was removed or expired.
    ShutdownNow,
}
