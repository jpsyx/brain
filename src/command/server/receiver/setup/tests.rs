use super::*;

#[test]
fn provider_setup_fields_follow_selected_channels() {
    assert_eq!(
        provider_fields(ReceiverSetupChannels::Sms),
        [
            "brain_receiver_public_url",
            "twilio_account_sid",
            "twilio_auth_token",
            "twilio_from_number",
        ]
    );
    assert_eq!(
        provider_fields(ReceiverSetupChannels::Email),
        [
            "brain_receiver_public_url",
            "resend_api_key",
            "resend_from_email",
            "resend_webhook_signing_secret",
        ]
    );
}

#[test]
fn provider_requirements_name_the_public_cli_flag() {
    assert_eq!(provider_cli_flag("brain_receiver_public_url"), "public-url");
    assert_eq!(provider_cli_flag("twilio_auth_token"), "twilio-auth-token");
}

fn plan(channels: ReceiverSetupChannels, providers: &[(&'static str, &str)]) -> SetupPlan {
    SetupPlan {
        channels,
        providers: providers
            .iter()
            .map(|(name, value)| (*name, (*value).to_owned()))
            .collect(),
        users: crate::users::Users::empty(),
    }
}

#[test]
fn shared_plan_validation_normalizes_public_url_and_provider_senders() {
    let mut plan = plan(
        ReceiverSetupChannels::Both,
        &[
            ("brain_receiver_public_url", "https://brain.example.test/"),
            ("twilio_account_sid", "AC123"),
            ("twilio_auth_token", "secret"),
            ("twilio_from_number", "(212) 555-0100"),
            ("resend_api_key", "re_secret"),
            ("resend_from_email", "Brain@Example.TEST"),
            ("resend_webhook_signing_secret", "whsec_secret"),
        ],
    );

    validate_plan(&mut plan).unwrap();

    assert_eq!(
        provider_value(&plan.providers, "brain_receiver_public_url"),
        Some("https://brain.example.test")
    );
    assert_eq!(
        provider_value(&plan.providers, "twilio_from_number"),
        Some("+12125550100")
    );
    assert_eq!(
        provider_value(&plan.providers, "resend_from_email"),
        Some("brain@example.test")
    );
}

#[test]
fn shared_plan_validation_rejects_malformed_bases_and_blank_required_values_without_echo() {
    for bad_url in [
        "http://brain.example.test",
        "https://",
        "https://brain.example.test/path",
        "https://brain.example.test?token=private",
        "https://brain.example.test#private",
    ] {
        let mut plan = plan(
            ReceiverSetupChannels::Sms,
            &[
                ("brain_receiver_public_url", bad_url),
                ("twilio_account_sid", "AC123"),
                ("twilio_auth_token", "secret"),
                ("twilio_from_number", "+12125550100"),
            ],
        );
        let error = validate_plan(&mut plan).unwrap_err().to_string();
        assert!(!error.contains(bad_url), "{error}");
    }

    let mut cleared = plan(
        ReceiverSetupChannels::Email,
        &[
            ("brain_receiver_public_url", ""),
            ("resend_api_key", "re_secret"),
            ("resend_from_email", "brain@example.test"),
            ("resend_webhook_signing_secret", "whsec_secret"),
        ],
    );
    assert!(validate_plan(&mut cleared).is_err());
}

#[test]
fn guided_clear_flows_into_the_shared_required_value_validation() {
    let cleared = resolve_provider_input("https://brain.example.test", "/clear");
    let mut plan = plan(
        ReceiverSetupChannels::Email,
        &[
            ("brain_receiver_public_url", &cleared),
            ("resend_api_key", "re_secret"),
            ("resend_from_email", "brain@example.test"),
            ("resend_webhook_signing_secret", "whsec_secret"),
        ],
    );

    let error = validate_plan(&mut plan).unwrap_err().to_string();

    assert!(error.contains("--public-url"), "{error}");
    assert!(!error.contains("brain.example.test"), "{error}");
}

#[test]
fn selected_channel_without_user_id_uses_guided_setup() {
    let args = crate::cli::ReceiverSetupArgs {
        channels: Some(ReceiverSetupChannels::Sms),
        ..Default::default()
    };

    assert!(!uses_headless_setup(&args));
}
