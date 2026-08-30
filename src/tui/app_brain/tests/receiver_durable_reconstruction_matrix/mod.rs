use super::*;

mod agent;
mod delivery;
pub(super) mod departure;

use departure::Departure;

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
fn orderly_shutdown_and_fresh_app_reconstruction_cover_every_durable_phase() {
    assert_reconstruction_matrix(Departure::Orderly);
}

#[test]
fn crash_drop_only_and_fresh_app_reconstruction_cover_every_durable_phase() {
    assert_reconstruction_matrix(Departure::Crash);
}

fn assert_reconstruction_matrix(departure: Departure) {
    for phase in RestartPhase::ALL {
        match phase {
            RestartPhase::Queued
            | RestartPhase::Claimed
            | RestartPhase::Launching
            | RestartPhase::Launched
            | RestartPhase::Accepted
            | RestartPhase::Processing
            | RestartPhase::Failed
            | RestartPhase::Done => agent::assert_reconstructs_and_advances(phase, departure),
            RestartPhase::Recovery => {
                super::receiver_recovery_frontend_matrix::assert_reconstructed_frontend_recovery_matrix(departure);
            }
            RestartPhase::CleanupGated
            | RestartPhase::AnswerReady
            | RestartPhase::Delivering
            | RestartPhase::Retrying
            | RestartPhase::Acknowledged => {
                delivery::assert_reconstructs_and_advances(phase, departure);
            }
        }
    }
}
