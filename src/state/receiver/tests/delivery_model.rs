const DELIVERY_PRIVATE_BODY: &str = "private-delivery-body-a101";
const DELIVERY_PRIVATE_RECIPIENT: &str = "private-delivery-recipient-b202@example.test";
const DELIVERY_PRIVATE_MESSAGE_ID: &str = "private-delivery-message-c303@example.test";

#[test]
fn serialized_delivery_envelopes_require_and_preserve_the_exact_outbound_sender() {
    for encoded in [
        serde_json::json!({
            "channel": "sms",
            "value": {
                "sender": "+12125550100",
                "recipient": "+12125550199",
                "body": "safe",
                "long_form_available": false
            }
        }),
        serde_json::json!({
            "channel": "email",
            "value": {
                "sender": "brain@example.test",
                "recipients": ["member@example.test"],
                "subject": "Re: Question",
                "text": "safe",
                "html": "<p>safe</p>",
                "in_reply_to": null,
                "references": null,
                "provider_email_id": "provider-email"
            }
        }),
    ] {
        let envelope = serde_json::from_value::<ReceiverDeliveryEnvelope>(encoded.clone())
            .expect("sender is immutable envelope routing data");
        assert!(
            serde_json::to_value(envelope).expect("serialize immutable envelope") == encoded,
            "serialized immutable envelope changed"
        );
    }
}

#[test]
fn email_delivery_render_rejects_noncanonical_persisted_sender_forms() {
    let job = email_delivery_job();

    for sender in [
        "Brain@Example.Test",
        " Brain@example.test ",
        "Brain <brain@example.test>",
        "brain@example.test>",
        ".brain@example.test",
        "brain.@example.test",
        "brain..reply@example.test",
        "brain@-example.test",
        "brain@example-.test",
        "brain@example..test",
        "brain@example_test",
    ] {
        let result = render_receiver_delivery(
            &job,
            ReceiverResponseKind::FinalAnswer,
            sender,
            DELIVERY_PRIVATE_BODY,
        );

        assert!(
            matches!(
                result,
                Err(ReceiverDeliveryRenderError::InvalidOutboundSender)
            ),
            "noncanonical persisted email sender was rendered"
        );
    }
}

#[test]
fn serialized_email_envelope_rejects_noncanonical_sender_forms() {
    for sender in [
        "Brain@Example.Test",
        " Brain@example.test ",
        "Brain <brain@example.test>",
        "brain@example.test>",
        ".brain@example.test",
        "brain.@example.test",
        "brain..reply@example.test",
        "brain@-example.test",
        "brain@example-.test",
        "brain@example..test",
        "brain@example_test",
    ] {
        let encoded = serde_json::json!({
            "channel": "email",
            "value": {
                "sender": sender,
                "recipients": ["member@example.test"],
                "subject": "Re: Question",
                "text": "safe",
                "html": "<p>safe</p>",
                "in_reply_to": null,
                "references": null,
                "provider_email_id": "provider-email"
            }
        });

        assert!(
            serde_json::from_value::<ReceiverDeliveryEnvelope>(encoded).is_err(),
            "noncanonical persisted email sender decoded"
        );
    }
}

fn email_delivery_job() -> crate::server::receiver::InboundJob {
    let mut job = receiver_job_for(
        receiver_workspace_id(),
        crate::server::receiver::Channel::Email,
        Some("provider-email"),
        100,
    );
    job.response_email = Some("Primary <primary@example.test>".to_owned());
    job.allowed_response_recipients = vec![
        "Copy <copy@example.test>".to_owned(),
        "PRIMARY@example.test".to_owned(),
    ];
    job.thread_participants = vec!["outsider@example.test".to_owned()];
    job.email_reply = Some(crate::server::receiver::EmailReplyContext {
        provider_email_id: "provider-email".to_owned(),
        subject: "Question".to_owned(),
        message_id: Some(DELIVERY_PRIVATE_MESSAGE_ID.to_owned()),
    });
    job
}

#[test]
fn sms_delivery_freezes_the_accepted_sender_and_existing_length_behavior() {
    let mut job = receiver_job(Some("provider-sms"), 100);
    job.authenticated_sender = "+12125550199".to_owned();
    let answer = format!("**{}**", "x".repeat(crate::server::reply::SMS_LIMIT + 40));

    let envelope = render_receiver_delivery(
        &job,
        ReceiverResponseKind::FinalAnswer,
        "+12125550100",
        &answer,
    )
    .expect("render immutable SMS delivery");
    let sms = envelope.sms().expect("SMS envelope");

    assert!(sms.sender() == "+12125550100", "SMS sender changed");
    assert!(
        sms.recipient() == "+12125550199",
        "SMS recipient changed"
    );
    assert!(sms.long_form_available());
    assert!(sms.body().chars().count() <= crate::server::reply::SMS_LIMIT);
    assert!(!sms.body().contains("**"));
}

#[test]
fn email_delivery_freezes_only_acceptance_time_authorized_recipients_and_lineage() {
    let job = email_delivery_job();

    let envelope = render_receiver_delivery(
        &job,
        ReceiverResponseKind::FinalAnswer,
        "brain@example.test",
        "  ## Answer\n\nDetails  ",
    )
    .expect("render immutable email delivery");
    let email = envelope.email().expect("email envelope");

    assert!(email.sender() == "brain@example.test", "email sender changed");
    assert!(
        email.recipients() == ["copy@example.test", "primary@example.test"],
        "email recipients changed"
    );
    assert!(email.subject() == "Re: Question", "email subject changed");
    assert!(email.text() == "## Answer\n\nDetails", "email text changed");
    assert!(email.html().contains("<h2>Answer</h2>"));
    assert!(
        email.in_reply_to() == Some(DELIVERY_PRIVATE_MESSAGE_ID),
        "email reply lineage changed"
    );
    assert!(
        email.references() == Some(DELIVERY_PRIVATE_MESSAGE_ID),
        "email reference lineage changed"
    );
    assert!(
        email.provider_email_id() == Some("provider-email"),
        "email provider lineage changed"
    );
    assert!(!email.recipients().contains(&"outsider@example.test".to_owned()));
}

#[test]
fn email_delivery_rejects_blank_accepted_provider_lineage_before_persistence() {
    for provider_email_id in ["", " \t "] {
        let mut job = email_delivery_job();
        job.email_reply
            .as_mut()
            .expect("email reply context")
            .provider_email_id = provider_email_id.to_owned();

        let error = render_receiver_delivery(
            &job,
            ReceiverResponseKind::FinalAnswer,
            "brain@example.test",
            DELIVERY_PRIVATE_BODY,
        )
        .expect_err("blank provider lineage must fail before persistence");

        assert_eq!(
            error,
            ReceiverDeliveryRenderError::InvalidAcceptedEmailProviderId
        );
        assert_eq!(
            error.to_string(),
            "receiver delivery has an invalid accepted email provider ID"
        );
        let rendered = format!("{error:?} {error}");
        assert!(!rendered.contains(DELIVERY_PRIVATE_BODY));
        assert!(!rendered.contains(DELIVERY_PRIVATE_MESSAGE_ID));
    }
}

#[test]
fn email_delivery_rejects_blank_accepted_message_lineage_before_persistence() {
    for message_id in ["", " \t "] {
        let mut job = email_delivery_job();
        job.email_reply
            .as_mut()
            .expect("email reply context")
            .message_id = Some(message_id.to_owned());

        let error = render_receiver_delivery(
            &job,
            ReceiverResponseKind::FinalAnswer,
            "brain@example.test",
            DELIVERY_PRIVATE_BODY,
        )
        .expect_err("blank message lineage must fail before persistence");

        assert_eq!(
            error,
            ReceiverDeliveryRenderError::InvalidAcceptedEmailMessageId
        );
        assert_eq!(
            error.to_string(),
            "receiver delivery has an invalid accepted email message ID"
        );
        let rendered = format!("{error:?} {error}");
        assert!(!rendered.contains(DELIVERY_PRIVATE_BODY));
        assert!(!rendered.contains(DELIVERY_PRIVATE_MESSAGE_ID));
    }
}

#[test]
fn rendered_email_without_message_lineage_round_trips_through_validation() {
    let mut job = email_delivery_job();
    job.email_reply
        .as_mut()
        .expect("email reply context")
        .message_id = None;

    let envelope = render_receiver_delivery(
        &job,
        ReceiverResponseKind::FinalAnswer,
        "brain@example.test",
        DELIVERY_PRIVATE_BODY,
    )
    .expect("missing optional message lineage is allowed");
    let encoded = serde_json::to_string(&envelope).expect("serialize rendered envelope");
    let decoded: ReceiverDeliveryEnvelope =
        serde_json::from_str(&encoded).expect("reload rendered envelope");

    assert!(decoded == envelope, "rendered email envelope changed");
    let email = decoded.email().expect("email envelope");
    assert!(email.in_reply_to().is_none(), "email reply lineage was set");
    assert!(email.references().is_none(), "email references were set");
    assert!(
        email.provider_email_id() == Some("provider-email"),
        "email provider lineage changed"
    );
}

#[test]
fn email_delivery_rejects_an_empty_accepted_recipient_set_without_echoing_content() {
    let mut job = email_delivery_job();
    job.response_email = None;
    job.allowed_response_recipients.clear();

    let error = render_receiver_delivery(
        &job,
        ReceiverResponseKind::FinalAnswer,
        "brain@example.test",
        DELIVERY_PRIVATE_BODY,
    )
    .expect_err("empty trusted recipients must fail authorization");

    assert_eq!(error, ReceiverDeliveryRenderError::NoTrustedEmailRecipients);
    let rendered = format!("{error:?} {error}");
    assert!(!rendered.contains(DELIVERY_PRIVATE_BODY));
    assert!(!rendered.contains("not-an-email"));
}

#[test]
fn email_delivery_rejects_any_invalid_accepted_recipient_without_partial_delivery() {
    for (response_email, allowed_response_recipients) in [
        (
            Some("not-an-email".to_owned()),
            vec!["valid@example.test".to_owned()],
        ),
        (
            Some("valid@example.test".to_owned()),
            vec!["not-an-email".to_owned()],
        ),
    ] {
        let mut job = email_delivery_job();
        job.response_email = response_email;
        job.allowed_response_recipients = allowed_response_recipients;

        let error = render_receiver_delivery(
            &job,
            ReceiverResponseKind::FinalAnswer,
            "brain@example.test",
            DELIVERY_PRIVATE_BODY,
        )
        .expect_err("one malformed accepted recipient must reject the entire delivery");

        assert!(
            error.to_string() == "receiver delivery has an invalid accepted email recipient",
            "invalid recipient error category changed"
        );
        let rendered = format!("{error:?} {error}");
        assert!(!rendered.contains(DELIVERY_PRIVATE_BODY));
        assert!(!rendered.contains("not-an-email"));
    }
}

#[test]
fn sms_delivery_rejects_an_invalid_accepted_sender_without_echoing_it() {
    let mut job = receiver_job(Some("provider-sms"), 100);
    job.authenticated_sender = "private-invalid-sender".to_owned();

    let error = render_receiver_delivery(
        &job,
        ReceiverResponseKind::FinalAnswer,
        "+12125550100",
        DELIVERY_PRIVATE_BODY,
    )
    .expect_err("invalid accepted SMS sender must fail closed");

    assert_eq!(
        error.to_string(),
        "receiver delivery has an invalid accepted SMS recipient"
    );
    assert!(!format!("{error:?} {error}").contains("private-invalid-sender"));
}

#[test]
fn serialized_sms_envelope_rejects_invalid_recipient_and_oversized_body() {
    let oversized = "x".repeat(crate::server::reply::SMS_LIMIT + 1);
    let cases = [
        serde_json::json!({
            "channel": "sms",
            "value": {
                "sender": "+12125550100",
                "recipient": "private-invalid-sender",
                "body": "safe",
                "long_form_available": false
            }
        }),
        serde_json::json!({
            "channel": "sms",
            "value": {
                "sender": "+12125550100",
                "recipient": "+12125550199",
                "body": oversized,
                "long_form_available": true
            }
        }),
    ];

    for encoded in cases {
        let encoded = encoded.to_string();
        let error = serde_json::from_str::<ReceiverDeliveryEnvelope>(&encoded)
            .expect_err("malformed persisted SMS envelope must fail closed");
        let rendered = error.to_string();
        assert!(rendered.contains("receiver SMS delivery envelope is invalid"));
        assert!(!rendered.contains("private-invalid-sender"));
        assert!(!rendered.contains(&"x".repeat(64)));
    }
}

#[test]
fn serialized_email_envelope_rejects_malformed_frozen_invariants_without_echoing_content() {
    let base = serde_json::json!({
        "channel": "email",
        "value": {
            "sender": "brain@example.test",
            "recipients": ["member@example.test"],
            "subject": "Re: Question",
            "text": "Private answer",
            "html": "<p>Private answer</p>",
            "in_reply_to": "<message@example.test>",
            "references": "<message@example.test>",
            "provider_email_id": "provider-email"
        }
    });
    let mut cases = Vec::new();
    for (field, value) in [
        ("recipients", serde_json::json!([])),
        ("recipients", serde_json::json!(["not-an-email"])),
        (
            "recipients",
            serde_json::json!(["member@example.test", "member@example.test"]),
        ),
        ("subject", serde_json::json!("  ")),
        ("text", serde_json::json!(" Private answer ")),
        ("html", serde_json::json!("  ")),
        ("references", serde_json::json!("<different@example.test>")),
        ("provider_email_id", serde_json::json!("  ")),
    ] {
        let mut candidate = base.clone();
        candidate["value"][field] = value;
        cases.push(candidate);
    }

    for encoded in cases {
        let encoded = encoded.to_string();
        let error = serde_json::from_str::<ReceiverDeliveryEnvelope>(&encoded)
            .expect_err("malformed persisted email envelope must fail closed");
        let rendered = error.to_string();
        assert!(rendered.contains("receiver email delivery envelope is invalid"));
        for private in [
            "Private answer",
            "not-an-email",
            "different@example.test",
        ] {
            assert!(!rendered.contains(private));
        }
    }
}

#[test]
fn delivery_envelopes_round_trip_without_exposing_content_through_debug() {
    let mut job = email_delivery_job();
    job.response_email = Some(DELIVERY_PRIVATE_RECIPIENT.to_owned());
    job.allowed_response_recipients.clear();
    let envelope = render_receiver_delivery(
        &job,
        ReceiverResponseKind::FallbackNotice,
        "brain@example.test",
        DELIVERY_PRIVATE_BODY,
    )
    .expect("render private delivery");

    let encoded = serde_json::to_string(&envelope).expect("serialize delivery envelope");
    let decoded: ReceiverDeliveryEnvelope =
        serde_json::from_str(&encoded).expect("deserialize delivery envelope");

    assert!(decoded == envelope, "private delivery envelope changed");
    for rendered in [format!("{envelope:?}"), format!("{:?}", envelope.email())] {
        assert!(!rendered.contains(DELIVERY_PRIVATE_BODY));
        assert!(!rendered.contains(DELIVERY_PRIVATE_RECIPIENT));
        assert!(!rendered.contains(DELIVERY_PRIVATE_MESSAGE_ID));
    }
}

#[test]
fn delivery_identities_and_public_status_have_redacted_debug() {
    let delivery_id = ReceiverDeliveryId::parse("10000000-0000-4000-8000-000000000001")
        .expect("delivery ID");
    let attempt_id = ReceiverDeliveryAttemptId::parse("20000000-0000-4000-8000-000000000002")
        .expect("attempt ID");
    let provider_reference = ReceiverProviderReference::parse("provider-reference-private")
        .expect("provider reference");
    let status = ReceiverDeliveryStatus::new(
        delivery_id,
        ReceiverResponseKind::FinalAnswer,
        ReceiverDeliveryState::Retrying,
        2,
        Some(60_000),
        Some(ReceiverDeliveryErrorCategory::TransportUnavailable),
        None,
        true,
    );

    assert_eq!(delivery_id.to_string(), "10000000-0000-4000-8000-000000000001");
    assert_eq!(attempt_id.to_string(), "20000000-0000-4000-8000-000000000002");
    assert!(
        provider_reference.as_str() == "provider-reference-private",
        "provider reference changed"
    );
    assert_eq!(status.state(), ReceiverDeliveryState::Retrying);
    assert_eq!(status.attempt_count(), 2);
    assert!(status.has_provider_reference());
    for rendered in [
        format!("{delivery_id:?}"),
        format!("{attempt_id:?}"),
        format!("{provider_reference:?}"),
        format!("{status:?}"),
    ] {
        assert!(!rendered.contains("10000000"));
        assert!(!rendered.contains("20000000"));
        assert!(!rendered.contains("provider-reference-private"));
    }
}

#[test]
fn delivery_retry_metadata_retains_only_content_free_attempt_timing() {
    let metadata = ReceiverDeliveryRetryMetadata::new(2, Some(301_000), Some(1_000));

    assert_eq!(metadata.attempt_count(), 2);
    assert_eq!(metadata.retry_at_unix_ms(), Some(301_000));
    assert_eq!(metadata.first_attempt_at_unix_ms(), Some(1_000));
    assert_eq!(
        format!("{metadata:?}"),
        "ReceiverDeliveryRetryMetadata { attempt_count: 2, retry_at_unix_ms: Some(301000), first_attempt_at_unix_ms: Some(1000) }"
    );
}
