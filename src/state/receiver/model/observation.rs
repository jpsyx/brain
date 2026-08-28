use std::fmt::Formatter;

use super::{ReceiverJobId, ReceiverJobToken, ReceiverSessionAttribution};

/// One frontend-neutral nonterminal receiver lifecycle fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverNonterminalObservationPhase {
    Accepted,
    Progressing,
}

/// Content-free evidence and authorization timing for one post-spawn launch.
#[derive(Clone, PartialEq, Eq)]
pub struct ReceiverLaunchObservation {
    pub token: ReceiverJobToken,
    pub instance: String,
    pub session_id: String,
    pub observed_at_unix_ms: u64,
    pub authorized_at_unix_ms: u64,
}

impl std::fmt::Debug for ReceiverLaunchObservation {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiverLaunchObservation(<redacted>)")
    }
}

/// Content-free evidence and authorization timing for one nonterminal lifecycle fact.
#[derive(Clone, PartialEq, Eq)]
pub struct ReceiverObservation {
    pub token: ReceiverJobToken,
    pub instance: String,
    pub session_id: String,
    pub phase: ReceiverNonterminalObservationPhase,
    pub revision: u64,
    pub observed_at_unix_ms: u64,
    pub authorized_at_unix_ms: u64,
}

impl std::fmt::Debug for ReceiverObservation {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiverObservation(<redacted>)")
    }
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

    pub(crate) fn nonterminal_from_agent_observation(
        token: ReceiverJobToken,
        registration: &ReceiverSessionAttribution,
        result: &crate::agent::AgentObservationResult,
        authorized_at_unix_ms: u64,
    ) -> Option<Self> {
        let mut observation =
            Self::from_agent_observation(token, registration, result, authorized_at_unix_ms);
        if observation.completed_at_unix_ms.take().is_some() {
            observation.revision = observation.revision.saturating_sub(1);
        }
        (observation.accepted_at_unix_ms.is_some()
            || observation.progressing_at_unix_ms.is_some()
            || observation.latest_progress_at_unix_ms.is_some())
        .then_some(observation)
    }
}

/// Exact durable identity and timings required to complete one receiver job.
#[derive(Clone, Copy)]
pub struct ReceiverCompletionRequest<'a> {
    pub job_id: ReceiverJobId,
    pub token: ReceiverJobToken,
    pub owner: &'a str,
    pub registration: &'a ReceiverSessionAttribution,
    pub completed_session: &'a crate::agent::AgentSession,
    pub answer: &'a str,
    pub observed_at_unix_ms: u64,
    pub authorized_at_unix_ms: u64,
}

impl std::fmt::Debug for ReceiverCompletionRequest<'_> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiverCompletionRequest(<redacted>)")
    }
}

/// Durable answer commit accepted for the first time or matched idempotently.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ReceiverCompletionOutcome {
    delivery_id: super::ReceiverDeliveryId,
    newly_recorded: bool,
}

impl ReceiverCompletionOutcome {
    pub(in crate::state::receiver) const fn recorded(
        delivery_id: super::ReceiverDeliveryId,
    ) -> Self {
        Self {
            delivery_id,
            newly_recorded: true,
        }
    }

    pub(in crate::state::receiver) const fn existing(
        delivery_id: super::ReceiverDeliveryId,
    ) -> Self {
        Self {
            delivery_id,
            newly_recorded: false,
        }
    }

    #[must_use]
    pub const fn delivery_id(self) -> super::ReceiverDeliveryId {
        self.delivery_id
    }

    #[must_use]
    pub const fn newly_recorded(self) -> bool {
        self.newly_recorded
    }
}

impl std::fmt::Debug for ReceiverCompletionOutcome {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiverCompletionOutcome(<redacted>)")
    }
}
