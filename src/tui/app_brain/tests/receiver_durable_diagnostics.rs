use super::*;

use crate::agent::AgentObservationPhase;
use crate::state::ReceiverJobState;
use crate::tui::app_brain::receiver::diagnostic::receiver_observation_diagnostic;

#[test]
fn receiver_observation_diagnostics_have_one_stable_content_free_shape() {
    let job_id = crate::state::ReceiverJobId::from(
        uuid::Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap(),
    );

    let diagnostic = receiver_observation_diagnostic(
        job_id,
        "00000000-0000-4000-8000-000000000002",
        AgentKind::OpenCode,
        ReceiverJobState::Processing,
        None,
        "tab-identity-mismatch",
    );
    assert_eq!(
        diagnostic,
        "receiver observation job=00000000-0000-4000-8000-000000000001 instance=00000000-0000-4000-8000-000000000002 frontend=opencode prior=processing boundary=none category=tab-identity-mismatch"
    );
    for private in [
        "11111111-1111-4111-8111-111111111111",
        "prompt-canary-7e7b",
        "body-canary-8f8c",
        "response-canary-9a9d",
        "recipient-canary-acde",
        "credential-canary-bdef",
    ] {
        assert!(!diagnostic.contains(private), "diagnostic leaked {private}");
    }
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
