//! Provider-specific unavailable responses for work that cannot be accepted.

use std::path::Path;

use super::{InboundJob, forward_job};

const UNAVAILABLE_MESSAGE: &str =
    "Brain is unavailable for this workspace. Please try again when its TUI is open.";

/// One provider response emitted while the inbound job is discarded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnavailableResponse {
    pub channel: super::Channel,
    pub body: String,
}

/// Observable result of the non-retrying live-socket handoff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardOutcome {
    pub forwarded: bool,
    pub retry_scheduled: bool,
    pub responses: Vec<UnavailableResponse>,
}

/// Attempt one live-TUI handoff. A failure emits one response and discards.
#[must_use]
pub fn forward_or_unavailable(path: &Path, job: &InboundJob) -> ForwardOutcome {
    match forward_job(path, job) {
        Ok(()) => ForwardOutcome {
            forwarded: true,
            retry_scheduled: false,
            responses: Vec::new(),
        },
        Err(_) => ForwardOutcome {
            forwarded: false,
            retry_scheduled: false,
            responses: vec![UnavailableResponse {
                channel: job.channel,
                body: UNAVAILABLE_MESSAGE.to_owned(),
            }],
        },
    }
}

/// Concise provider-facing unavailable text.
#[must_use]
pub const fn message() -> &'static str {
    UNAVAILABLE_MESSAGE
}
