const DELIVERY_PRIVATE_BODY: &str = "private-delivery-body-a101";
const DELIVERY_PRIVATE_RECIPIENT: &str = "private-delivery-recipient-b202@example.test";
const DELIVERY_PRIVATE_MESSAGE_ID: &str = "private-delivery-message-c303@example.test";

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
        &answer,
    )
    .expect("render immutable SMS delivery");
    let sms = envelope.sms().expect("SMS envelope");

    assert_eq!(sms.recipient(), "+12125550199");
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
        "  ## Answer\n\nDetails  ",
    )
    .expect("render immutable email delivery");
    let email = envelope.email().expect("email envelope");

    assert_eq!(
        email.recipients(),
        ["copy@example.test", "primary@example.test"]
    );
    assert_eq!(email.subject(), "Re: Question");
    assert_eq!(email.text(), "## Answer\n\nDetails");
    assert!(email.html().contains("<h2>Answer</h2>"));
    assert_eq!(email.in_reply_to(), Some(DELIVERY_PRIVATE_MESSAGE_ID));
    assert_eq!(email.references(), Some(DELIVERY_PRIVATE_MESSAGE_ID));
    assert_eq!(email.provider_email_id(), Some("provider-email"));
    assert!(!email.recipients().contains(&"outsider@example.test".to_owned()));
}

#[test]
fn email_delivery_rejects_an_empty_accepted_recipient_set_without_echoing_content() {
    let mut job = email_delivery_job();
    job.response_email = Some("not-an-email".to_owned());
    job.allowed_response_recipients.clear();

    let error = render_receiver_delivery(
        &job,
        ReceiverResponseKind::FinalAnswer,
        DELIVERY_PRIVATE_BODY,
    )
    .expect_err("empty trusted recipients must fail authorization");

    assert_eq!(error, ReceiverDeliveryRenderError::NoTrustedEmailRecipients);
    let rendered = format!("{error:?} {error}");
    assert!(!rendered.contains(DELIVERY_PRIVATE_BODY));
    assert!(!rendered.contains("not-an-email"));
}

#[test]
fn delivery_envelopes_round_trip_without_exposing_content_through_debug() {
    let mut job = email_delivery_job();
    job.response_email = Some(DELIVERY_PRIVATE_RECIPIENT.to_owned());
    job.allowed_response_recipients.clear();
    let envelope = render_receiver_delivery(
        &job,
        ReceiverResponseKind::FallbackNotice,
        DELIVERY_PRIVATE_BODY,
    )
    .expect("render private delivery");

    let encoded = serde_json::to_string(&envelope).expect("serialize delivery envelope");
    let decoded: ReceiverDeliveryEnvelope =
        serde_json::from_str(&encoded).expect("deserialize delivery envelope");

    assert_eq!(decoded, envelope);
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
    assert_eq!(provider_reference.as_str(), "provider-reference-private");
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
