//! Provider-specific unavailable responses for work that cannot be accepted.

const UNAVAILABLE_MESSAGE: &str =
    "Brain is unavailable for this workspace. Please try again when its TUI is open.";

/// Concise provider-facing unavailable text.
#[must_use]
pub const fn message() -> &'static str {
    UNAVAILABLE_MESSAGE
}
