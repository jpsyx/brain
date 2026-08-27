use super::*;

#[test]
fn observation_result_debug_omits_the_native_session() {
    const PRIVATE_SESSION: &str = "observation-native-session-canary";
    let result = AgentObservationResult::new(
        AgentSession::new(PRIVATE_SESSION).expect("private observation session"),
        vec![AgentObservationBoundary::new(
            AgentObservationPhase::Accepted,
            1_000,
        )],
        Some(AgentProgressPulse::new(1_100)),
        AgentObservationCursor::at_revision(1, Some(1_000), None, None, None),
    );
    let rendered = format!("{result:?}");

    assert!(
        !rendered.contains(PRIVATE_SESSION),
        "observation result Debug contains a native session"
    );
    assert!(
        rendered == "AgentObservationResult(<redacted>)",
        "observation result Debug shape mismatch"
    );
}
