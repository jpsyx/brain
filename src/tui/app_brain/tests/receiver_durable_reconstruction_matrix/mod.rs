use super::*;

mod agent;
mod delivery;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RestartPhase {
    Queued,
    Claimed,
    Launching,
    Launched,
    Accepted,
    Processing,
    Recovery,
    CleanupGated,
    AnswerReady,
    Delivering,
    Retrying,
    Acknowledged,
    Failed,
    Done,
}

impl RestartPhase {
    const ALL: [Self; 14] = [
        Self::Queued,
        Self::Claimed,
        Self::Launching,
        Self::Launched,
        Self::Accepted,
        Self::Processing,
        Self::Recovery,
        Self::CleanupGated,
        Self::AnswerReady,
        Self::Delivering,
        Self::Retrying,
        Self::Acknowledged,
        Self::Failed,
        Self::Done,
    ];
}

#[test]
fn shutdown_and_fresh_app_reconstruction_cover_every_durable_phase() {
    for phase in RestartPhase::ALL {
        match phase {
            RestartPhase::Queued
            | RestartPhase::Claimed
            | RestartPhase::Launching
            | RestartPhase::Launched
            | RestartPhase::Accepted
            | RestartPhase::Processing
            | RestartPhase::Failed
            | RestartPhase::Done => agent::assert_reconstructs_and_advances(phase),
            RestartPhase::Recovery => {
                super::receiver_recovery_frontend_matrix::assert_reconstructed_frontend_recovery_matrix();
            }
            RestartPhase::CleanupGated
            | RestartPhase::AnswerReady
            | RestartPhase::Delivering
            | RestartPhase::Retrying
            | RestartPhase::Acknowledged => delivery::assert_reconstructs_and_advances(phase),
        }
    }
}
