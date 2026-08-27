use std::fmt::Formatter;

use super::{ReceiverJobId, ReceiverJobToken, ReceiverSessionAttribution};

/// One frontend-neutral nonterminal receiver lifecycle fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverNonterminalObservationPhase {
    Accepted,
    Progressing,
}

/// Content-free evidence and authorization timing for one post-spawn launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiverLaunchObservation {
    pub token: ReceiverJobToken,
    pub instance: String,
    pub session_id: String,
    pub observed_at_unix_ms: u64,
    pub authorized_at_unix_ms: u64,
}

/// Content-free evidence and authorization timing for one nonterminal lifecycle fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiverObservation {
    pub token: ReceiverJobToken,
    pub instance: String,
    pub session_id: String,
    pub phase: ReceiverNonterminalObservationPhase,
    pub revision: u64,
    pub observed_at_unix_ms: u64,
    pub authorized_at_unix_ms: u64,
}

/// Every newly represented lifecycle boundary from one normalized snapshot.
#[derive(Clone, PartialEq, Eq)]
pub struct ReceiverObservationSet {
    pub token: ReceiverJobToken,
    pub instance: String,
    pub session_id: String,
    pub revision: u64,
    pub accepted_at_unix_ms: Option<u64>,
    pub progressing_at_unix_ms: Option<u64>,
    pub latest_progress_at_unix_ms: Option<u64>,
    pub completed_at_unix_ms: Option<u64>,
    pub authorized_at_unix_ms: u64,
}

impl std::fmt::Debug for ReceiverObservationSet {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiverObservationSet(<redacted>)")
    }
}

impl ReceiverObservationSet {
    pub(crate) fn from_agent_observation(
        token: ReceiverJobToken,
        registration: &ReceiverSessionAttribution,
        result: &crate::agent::AgentObservationResult,
        authorized_at_unix_ms: u64,
    ) -> Self {
        let mut accepted_at_unix_ms = None;
        let mut progressing_at_unix_ms = None;
        let mut completed_at_unix_ms = None;
        for boundary in result.boundaries() {
            match boundary.phase() {
                crate::agent::AgentObservationPhase::Launched => {}
                crate::agent::AgentObservationPhase::Accepted => {
                    accepted_at_unix_ms = Some(boundary.observed_at_unix_ms());
                }
                crate::agent::AgentObservationPhase::Progressing => {
                    progressing_at_unix_ms = Some(boundary.observed_at_unix_ms());
                }
                crate::agent::AgentObservationPhase::Completed => {
                    completed_at_unix_ms = Some(boundary.observed_at_unix_ms());
                }
            }
        }
        Self {
            token,
            instance: registration.instance().to_owned(),
            session_id: result.session().as_str().to_owned(),
            revision: result.next_cursor().durable_revision(),
            accepted_at_unix_ms,
            progressing_at_unix_ms,
            latest_progress_at_unix_ms: result
                .progress_pulse()
                .map(crate::agent::AgentProgressPulse::observed_at_unix_ms),
            completed_at_unix_ms,
            authorized_at_unix_ms,
        }
    }
}

/// Exact durable identity and timings required to complete one receiver job.
#[derive(Debug, Clone, Copy)]
pub struct ReceiverCompletionRequest<'a> {
    pub job_id: ReceiverJobId,
    pub token: ReceiverJobToken,
    pub owner: &'a str,
    pub registration: &'a ReceiverSessionAttribution,
    pub completed_session: &'a crate::agent::AgentSession,
    pub observed_at_unix_ms: u64,
    pub authorized_at_unix_ms: u64,
}
