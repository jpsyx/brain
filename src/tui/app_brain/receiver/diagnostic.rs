//! Stable, content-free diagnostics for receiver observation outcomes.

use crate::agent::{AgentKind, AgentObservationPhase};
use crate::state::{ReceiverJobId, ReceiverJobState};

pub(in crate::tui::app_brain) fn receiver_observation_diagnostic(
    job_id: ReceiverJobId,
    instance: &str,
    frontend: AgentKind,
    prior: ReceiverJobState,
    boundary: Option<AgentObservationPhase>,
    category: &'static str,
) -> String {
    format!(
        "receiver observation job={job_id} instance={instance} frontend={} prior={} boundary={} category={category}",
        frontend.as_str(),
        receiver_state_label(prior),
        boundary.map_or("none", observation_phase_label),
    )
}

const fn receiver_state_label(state: ReceiverJobState) -> &'static str {
    match state {
        ReceiverJobState::Queued => "queued",
        ReceiverJobState::Claimed => "claimed",
        ReceiverJobState::Launching => "launching",
        ReceiverJobState::Launched => "launched",
        ReceiverJobState::Accepted => "accepted",
        ReceiverJobState::Processing => "processing",
        ReceiverJobState::AnswerReady => "answer-ready",
        ReceiverJobState::Delivering => "delivering",
        ReceiverJobState::Retrying => "retrying",
        ReceiverJobState::Failed => "failed",
        ReceiverJobState::Done => "done",
    }
}

const fn observation_phase_label(phase: AgentObservationPhase) -> &'static str {
    match phase {
        AgentObservationPhase::Launched => "launched",
        AgentObservationPhase::Accepted => "accepted",
        AgentObservationPhase::Progressing => "progressing",
        AgentObservationPhase::Completed => "completed",
    }
}
