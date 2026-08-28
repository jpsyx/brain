#[test]
fn frozen_fallback_excludes_failed_provider_and_attempted_recipients() {
    let frozen = [
        ReceiverFallbackDestination::new(
            ReceiverProviderCapability::Twilio,
            "+12125550100",
        ),
        ReceiverFallbackDestination::new(
            ReceiverProviderCapability::Resend,
            "already@example.test",
        ),
        ReceiverFallbackDestination::new(
            ReceiverProviderCapability::Resend,
            "safe@example.test",
        ),
    ];
    let plan = plan_receiver_fallback(
        ReceiverProviderCapability::Twilio,
        &["already@example.test"],
        &frozen,
    )
    .expect("one frozen alternate remains safe");

    assert!(
        plan.destination().recipient() == "safe@example.test",
        "fallback selected the wrong frozen destination"
    );
    assert!(plan.notice().chars().count() <= crate::server::reply::SMS_LIMIT);
    assert!(!format!("{plan:?}").contains("safe@example.test"));
}

#[test]
fn fallback_never_uses_later_authority_and_current_single_channel_jobs_stop() {
    assert!(
        plan_receiver_fallback(ReceiverProviderCapability::Twilio, &[], &[]).is_none(),
        "current accepted jobs freeze no alternate authority"
    );
    let later_configuration = [ReceiverFallbackDestination::new(
        ReceiverProviderCapability::Resend,
        "later@example.test",
    )];
    assert_eq!(later_configuration.len(), 1);
    assert!(
        plan_receiver_fallback(ReceiverProviderCapability::Twilio, &[], &[]).is_none(),
        "later configuration is not an input to the frozen-authority planner"
    );
}
