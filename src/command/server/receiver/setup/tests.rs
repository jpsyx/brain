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
