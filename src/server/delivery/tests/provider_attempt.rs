use super::super::*;

#[test]
fn provider_success_requires_exact_resend_and_twilio_identifiers() {
    let resend = classify_provider_http_response(
        crate::state::ReceiverProviderCapability::Resend,
        200,
        br#"{"id":"10000000-0000-4000-8000-000000000001"}"#,
    );
    let twilio = classify_provider_http_response(
        crate::state::ReceiverProviderCapability::Twilio,
        201,
        br#"{"sid":"SM0123456789abcdef0123456789abcdef","status":"queued"}"#,
    );

    assert!(matches!(
        resend,
        crate::state::ReceiverProviderResultClass::Acknowledged(reference)
            if reference.as_str() == "10000000-0000-4000-8000-000000000001"
    ));
    assert!(matches!(
        twilio,
        crate::state::ReceiverProviderResultClass::Acknowledged(reference)
            if reference.as_str() == "SM0123456789abcdef0123456789abcdef"
    ));

    for (provider, body) in [
        (
            crate::state::ReceiverProviderCapability::Resend,
            br#"{"id":"not-an-email-id"}"#.as_slice(),
        ),
        (
            crate::state::ReceiverProviderCapability::Twilio,
            br#"{"sid":"SM-not-a-message-sid"}"#.as_slice(),
        ),
        (
            crate::state::ReceiverProviderCapability::Twilio,
            br#"{"status":"queued"}"#.as_slice(),
        ),
    ] {
        assert_eq!(
            classify_provider_http_response(provider, 200, body),
            crate::state::ReceiverProviderResultClass::Ambiguous(
                crate::state::ReceiverDeliveryAmbiguity::ProviderAcknowledgementMalformed
            )
        );
    }
}

#[test]
fn http_and_local_failures_follow_the_redacted_provider_matrix() {
    use crate::state::{
        ReceiverDeliveryAmbiguity as Ambiguity, ReceiverDeliveryErrorCategory as Error,
        ReceiverProviderCapability as Provider, ReceiverProviderResultClass as ResultClass,
    };

    assert_eq!(
        classify_provider_http_response(Provider::Resend, 429, b"private response"),
        ResultClass::DefinitelyNotAccepted(Error::TransportUnavailable)
    );
    for status in [500, 503] {
        assert_eq!(
            classify_provider_http_response(Provider::Resend, status, b"private response"),
            ResultClass::Ambiguous(Ambiguity::ProviderAcceptanceUnknown),
            "Resend can replay the frozen request with its delivery idempotency key"
        );
        assert_eq!(
            classify_provider_http_response(
                Provider::Twilio,
                status,
                br#"{"code":20500,"message":"private response"}"#,
            ),
            ResultClass::Ambiguous(Ambiguity::ProviderAcceptanceUnknown),
            "Twilio documents that retrying an SMS POST after 20500 can duplicate it"
        );
    }
    for status in [400, 401, 403, 422] {
        assert_eq!(
            classify_provider_http_response(Provider::Twilio, status, b"private response"),
            ResultClass::PermanentlyRejected(Error::ProviderRejected)
        );
    }
    for failure in [
        ReceiverProviderProcessFailure::Timeout,
        ReceiverProviderProcessFailure::ProcessExit,
        ReceiverProviderProcessFailure::Cancelled,
        ReceiverProviderProcessFailure::LostResultChannel,
    ] {
        assert_eq!(
            classify_provider_process_failure(failure),
            ResultClass::Ambiguous(Ambiguity::ProviderAcceptanceUnknown)
        );
    }
    assert_eq!(
        classify_provider_process_failure(ReceiverProviderProcessFailure::Spawn),
        ResultClass::DefinitelyNotAccepted(Error::TransportUnavailable)
    );
    assert_eq!(
        classify_provider_process_failure(ReceiverProviderProcessFailure::Credentials),
        ResultClass::PermanentlyRejected(Error::Credentials)
    );
    assert_eq!(
        classify_provider_process_failure(ReceiverProviderProcessFailure::Preflight),
        ResultClass::PermanentlyRejected(Error::InvalidRequest)
    );
    assert_eq!(
        classify_provider_process_failure(ReceiverProviderProcessFailure::ResponseTooLarge),
        ResultClass::Ambiguous(Ambiguity::ProviderAcknowledgementMalformed)
    );
}

#[test]
fn resend_409_distinguishes_concurrent_replay_from_a_changed_payload() {
    use crate::state::{
        ReceiverDeliveryAmbiguity as Ambiguity, ReceiverDeliveryErrorCategory as Error,
        ReceiverProviderCapability as Provider, ReceiverProviderResultClass as ResultClass,
    };

    assert_eq!(
        classify_provider_http_response(
            Provider::Resend,
            409,
            br#"{"name":"concurrent_idempotent_requests","message":"private"}"#,
        ),
        ResultClass::Ambiguous(Ambiguity::ProviderAcceptanceUnknown)
    );
    assert_eq!(
        classify_provider_http_response(
            Provider::Resend,
            409,
            br#"{"name":"invalid_idempotent_request","message":"private"}"#,
        ),
        ResultClass::PermanentlyRejected(Error::ProviderRejected)
    );
    assert_eq!(
        classify_provider_http_response(
            Provider::Resend,
            409,
            br#"{"name":"unknown-private-error","message":"private"}"#,
        ),
        ResultClass::PermanentlyRejected(Error::ProviderRejected)
    );
}

#[test]
fn nonzero_provider_process_cannot_acknowledge_an_apparent_success_body() {
    let output = b"{\"id\":\"10000000-0000-4000-8000-000000000001\"}\n__brain_http_status__:200";

    assert_eq!(
        classify_provider_process_output(
            crate::state::ReceiverProviderCapability::Resend,
            false,
            Some(23),
            output,
        ),
        crate::state::ReceiverProviderResultClass::Ambiguous(
            crate::state::ReceiverDeliveryAmbiguity::ProviderAcceptanceUnknown
        )
    );
}

#[test]
fn curl_resolution_and_connect_failures_are_safe_retries_for_both_providers() {
    use crate::state::{
        ReceiverDeliveryDecision as Decision, ReceiverDeliveryErrorCategory as Error,
        ReceiverDeliveryPolicySnapshot, ReceiverProviderCapability as Provider,
        ReceiverProviderResultClass as ResultClass, decide_receiver_delivery,
    };

    for provider in [Provider::Twilio, Provider::Resend] {
        for exit_code in [5, 6, 7] {
            let result = classify_provider_process_output(provider, false, Some(exit_code), b"");
            assert!(
                result == ResultClass::DefinitelyNotAccepted(Error::TransportUnavailable),
                "curl resolution/connect failure was not classified as pre-provider"
            );
            assert!(matches!(
                decide_receiver_delivery(ReceiverDeliveryPolicySnapshot {
                    provider,
                    attempt_count: 1,
                    first_attempt_at_unix_ms: Some(1_000),
                    now_unix_ms: 2_000,
                    result,
                }),
                Decision::RetryAt {
                    retry_at_unix_ms: 62_000,
                    error_category: Error::TransportUnavailable,
                }
            ));
        }
    }
}

#[test]
fn neighboring_curl_failures_remain_conservatively_ambiguous() {
    use crate::state::{
        ReceiverDeliveryAmbiguity as Ambiguity, ReceiverProviderCapability as Provider,
        ReceiverProviderResultClass as ResultClass,
    };

    for provider in [Provider::Twilio, Provider::Resend] {
        for exit_code in [4, 8, 28] {
            assert!(
                classify_provider_process_output(provider, false, Some(exit_code), b"")
                    == ResultClass::Ambiguous(Ambiguity::ProviderAcceptanceUnknown),
                "a curl failure without definite pre-provider semantics became retry-safe"
            );
        }
    }
}

#[test]
fn oversized_or_malformed_success_is_ambiguous_without_echoing_the_body() {
    let private = "private-provider-response";
    let oversized = vec![b'x'; PROVIDER_RESPONSE_LIMIT + 1];

    for body in [private.as_bytes(), oversized.as_slice()] {
        let result = classify_provider_http_response(
            crate::state::ReceiverProviderCapability::Resend,
            200,
            body,
        );
        assert_eq!(
            result,
            crate::state::ReceiverProviderResultClass::Ambiguous(
                crate::state::ReceiverDeliveryAmbiguity::ProviderAcknowledgementMalformed
            )
        );
        assert!(!format!("{result:?}").contains(private));
    }
}

#[test]
fn resend_replay_uses_the_exact_delivery_key_and_byte_identical_payload() {
    let delivery_id =
        crate::state::ReceiverDeliveryId::parse("10000000-0000-4000-8000-000000000001")
            .expect("delivery ID");
    let envelope: crate::state::ReceiverDeliveryEnvelope =
        serde_json::from_value(serde_json::json!({
            "channel": "email",
            "value": {
                "sender": "brain@example.test",
                "recipients": ["member@example.test"],
                "subject": "Re: Exact subject",
                "text": "private exact text",
                "html": "<p>private exact text</p>",
                "in_reply_to": "<message@example.test>",
                "references": "<message@example.test>",
                "provider_email_id": "provider-email-id"
            }
        }))
        .expect("frozen email envelope");

    let first = resend_request_for_test("secret", delivery_id, &envelope).expect("first request");
    let replay = resend_request_for_test("secret", delivery_id, &envelope).expect("replay request");

    assert_eq!(first, replay);
    assert!(first.has_exact_delivery_key());
    assert_eq!(first.idempotency_header_count(), 1);
    assert!(first.uses_frozen_sender());
    assert!(first.has_one_authorization_header());
    assert_eq!(format!("{first:?}"), "ResendRequestProof(<redacted>)");
}
