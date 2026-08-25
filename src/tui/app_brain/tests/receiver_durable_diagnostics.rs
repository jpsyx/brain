use super::*;

use crate::agent::AgentObservationPhase;
use crate::state::ReceiverJobState;
use crate::tui::app_brain::receiver::diagnostic::receiver_observation_diagnostic;

#[test]
fn receiver_observation_diagnostics_have_one_stable_content_free_shape() {
    let job_id = crate::state::ReceiverJobId::from(
        uuid::Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap(),
    );

    assert_eq!(
        receiver_observation_diagnostic(
            job_id,
            "00000000-0000-4000-8000-000000000002",
            AgentKind::OpenCode,
            ReceiverJobState::Processing,
            None,
            "tab-identity-mismatch",
        ),
        "receiver observation job=00000000-0000-4000-8000-000000000001 instance=00000000-0000-4000-8000-000000000002 frontend=opencode prior=processing boundary=none category=tab-identity-mismatch"
    );
    assert_eq!(
        receiver_observation_diagnostic(
            job_id,
            "00000000-0000-4000-8000-000000000002",
            AgentKind::Claude,
            ReceiverJobState::Accepted,
            Some(AgentObservationPhase::Completed),
            "persisted-terminal",
        ),
        "receiver observation job=00000000-0000-4000-8000-000000000001 instance=00000000-0000-4000-8000-000000000002 frontend=claude prior=accepted boundary=completed category=persisted-terminal"
    );
}
