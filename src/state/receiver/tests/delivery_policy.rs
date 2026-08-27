fn policy(
    provider: ReceiverProviderCapability,
    attempt_count: u32,
    first_attempt_at_unix_ms: Option<u64>,
    now_unix_ms: u64,
    result: ReceiverProviderResultClass,
) -> ReceiverDeliveryDecision {
    decide_receiver_delivery(ReceiverDeliveryPolicySnapshot {
        provider,
        attempt_count,
        first_attempt_at_unix_ms,
        now_unix_ms,
        result,
    })
}

#[test]
fn acknowledged_result_finishes_with_the_redacted_provider_reference() {
    let reference = ReceiverProviderReference::parse("provider-ack-private")
        .expect("provider reference");

    let decision = policy(
        ReceiverProviderCapability::Twilio,
        1,
        Some(100),
        200,
        ReceiverProviderResultClass::Acknowledged(reference.clone()),
    );

    assert_eq!(decision, ReceiverDeliveryDecision::Acknowledged(reference));
    assert!(!format!("{decision:?}").contains("provider-ack-private"));
}

#[test]
fn definitely_unaccepted_results_use_all_three_bounded_retry_delays() {
    for (attempt_count, expected_at) in [(1, 61_000), (2, 301_000), (3, 1_801_000)] {
        assert_eq!(
            policy(
                ReceiverProviderCapability::Twilio,
                attempt_count,
                Some(1_000),
                1_000,
                ReceiverProviderResultClass::DefinitelyNotAccepted(
                    ReceiverDeliveryErrorCategory::TransportUnavailable,
                ),
            ),
            ReceiverDeliveryDecision::RetryAt {
                retry_at_unix_ms: expected_at,
                error_category: ReceiverDeliveryErrorCategory::TransportUnavailable,
            }
        );
    }
}

#[test]
fn fourth_failed_attempt_exhausts_the_finite_budget() {
    assert_eq!(
        policy(
            ReceiverProviderCapability::Resend,
            4,
            Some(1_000),
            2_000,
            ReceiverProviderResultClass::DefinitelyNotAccepted(
                ReceiverDeliveryErrorCategory::TransportUnavailable,
            ),
        ),
        ReceiverDeliveryDecision::TerminalFailure(
            ReceiverDeliveryErrorCategory::RetryExhausted
        )
    );
}

#[test]
fn retry_deadlines_saturate_and_are_due_at_exact_equality() {
    let decision = policy(
        ReceiverProviderCapability::Twilio,
        3,
        Some(u64::MAX - 10),
        u64::MAX - 10,
        ReceiverProviderResultClass::DefinitelyNotAccepted(
            ReceiverDeliveryErrorCategory::TransportUnavailable,
        ),
    );

    assert_eq!(
        decision,
        ReceiverDeliveryDecision::RetryAt {
            retry_at_unix_ms: u64::MAX,
            error_category: ReceiverDeliveryErrorCategory::TransportUnavailable,
        }
    );
    assert!(!receiver_delivery_retry_is_due(u64::MAX - 1, u64::MAX));
    assert!(receiver_delivery_retry_is_due(u64::MAX, u64::MAX));
}

#[test]
fn permanent_rejection_is_terminal_without_consuming_more_attempts() {
    assert_eq!(
        policy(
            ReceiverProviderCapability::Resend,
            1,
            Some(1_000),
            1_001,
            ReceiverProviderResultClass::PermanentlyRejected(
                ReceiverDeliveryErrorCategory::Credentials,
            ),
        ),
        ReceiverDeliveryDecision::TerminalFailure(ReceiverDeliveryErrorCategory::Credentials)
    );
}

#[test]
fn twilio_ambiguity_is_terminal_because_create_has_no_idempotency_key() {
    assert_eq!(
        policy(
            ReceiverProviderCapability::Twilio,
            1,
            Some(1_000),
            1_001,
            ReceiverProviderResultClass::Ambiguous(
                ReceiverDeliveryAmbiguity::ProviderAcceptanceUnknown,
            ),
        ),
        ReceiverDeliveryDecision::TerminalAmbiguous(
            ReceiverDeliveryAmbiguity::ProviderAcceptanceUnknown
        )
    );
}

#[test]
fn resend_ambiguity_retries_within_and_at_the_24_hour_boundary() {
    let day = 24 * 60 * 60 * 1_000;
    for now in [1_000 + day - 1, 1_000 + day] {
        assert!(matches!(
            policy(
                ReceiverProviderCapability::Resend,
                1,
                Some(1_000),
                now,
                ReceiverProviderResultClass::Ambiguous(
                    ReceiverDeliveryAmbiguity::ProviderAcceptanceUnknown,
                ),
            ),
            ReceiverDeliveryDecision::RetryAt { .. }
        ));
    }
}

#[test]
fn resend_ambiguity_after_24_hours_is_explicitly_terminal() {
    let after_day = 1_000 + 24 * 60 * 60 * 1_000 + 1;

    assert_eq!(
        policy(
            ReceiverProviderCapability::Resend,
            1,
            Some(1_000),
            after_day,
            ReceiverProviderResultClass::Ambiguous(
                ReceiverDeliveryAmbiguity::ProviderAcceptanceUnknown,
            ),
        ),
        ReceiverDeliveryDecision::TerminalAmbiguous(
            ReceiverDeliveryAmbiguity::IdempotencyWindowExpired
        )
    );
}

#[test]
fn resend_ambiguity_without_a_first_attempt_time_fails_closed() {
    assert_eq!(
        policy(
            ReceiverProviderCapability::Resend,
            1,
            None,
            1_000,
            ReceiverProviderResultClass::Ambiguous(
                ReceiverDeliveryAmbiguity::ProviderAcceptanceUnknown,
            ),
        ),
        ReceiverDeliveryDecision::TerminalAmbiguous(
            ReceiverDeliveryAmbiguity::ProviderAcceptanceUnknown
        )
    );
}

#[test]
fn zero_attempts_is_an_invalid_terminal_policy_input() {
    assert_eq!(
        policy(
            ReceiverProviderCapability::Resend,
            0,
            None,
            1_000,
            ReceiverProviderResultClass::DefinitelyNotAccepted(
                ReceiverDeliveryErrorCategory::TransportUnavailable,
            ),
        ),
        ReceiverDeliveryDecision::TerminalFailure(
            ReceiverDeliveryErrorCategory::InvalidRequest
        )
    );
}
