use brain::workspace::{FeatureStatus, RequirementScope, requirements};
use serde_json::{Map, Value, json};

use super::support::{Fixture, feature_status};

#[test]
fn enabled_receiver_with_partial_sms_configuration_is_incomplete() {
    let fixture = Fixture::with_receiver(
        Map::from_iter([(
            "twilio_auth_token".to_owned(),
            Value::String("secret-must-never-render".to_owned()),
        )]),
        true,
    );

    let health = requirements(&fixture.command).expect("inspect selected workspace");

    assert_eq!(
        feature_status(&health, &RequirementScope::Receiver),
        FeatureStatus::Incomplete
    );
    assert_eq!(
        feature_status(&health, &RequirementScope::Sms),
        FeatureStatus::Incomplete
    );
    assert_eq!(
        feature_status(&health, &RequirementScope::Email),
        FeatureStatus::Off
    );
}

#[test]
fn malformed_present_receiver_field_is_incomplete_instead_of_off() {
    let fixture = Fixture::with_receiver(
        Map::from_iter([("twilio_auth_token".to_owned(), json!(42))]),
        true,
    );

    let health = requirements(&fixture.command).expect("inspect selected workspace");

    assert_eq!(
        feature_status(&health, &RequirementScope::Sms),
        FeatureStatus::Incomplete
    );
}

#[test]
fn complete_sms_uses_portable_mapping_and_leaves_email_off() {
    let fixture = Fixture::with_receiver(
        Map::from_iter([
            (
                "brain_receiver_public_url".to_owned(),
                json!("https://receiver.example"),
            ),
            ("twilio_account_sid".to_owned(), json!("account")),
            ("twilio_auth_token".to_owned(), json!("secret")),
            ("twilio_from_number".to_owned(), json!("+15557654321")),
        ]),
        true,
    );
    fixture.write_users(
        r#"{
          "schema_version": 1,
          "users": [{
            "id": "pablo",
            "name": "Pablo",
            "phones": [{"value": "+15551234567", "inbound_allowed": true}],
            "emails": [],
            "response_email": null
          }]
        }"#,
    );

    let health = requirements(&fixture.command).expect("inspect selected workspace");

    assert_eq!(
        feature_status(&health, &RequirementScope::Receiver),
        FeatureStatus::Ready
    );
    assert_eq!(
        feature_status(&health, &RequirementScope::Sms),
        FeatureStatus::Ready
    );
    assert_eq!(
        feature_status(&health, &RequirementScope::Email),
        FeatureStatus::Off
    );
}

#[test]
fn complete_email_uses_portable_mapping_and_leaves_sms_off() {
    let fixture = Fixture::with_receiver(
        Map::from_iter([
            (
                "brain_receiver_public_url".to_owned(),
                json!("https://receiver.example"),
            ),
            ("resend_sending_api_key".to_owned(), json!("secret")),
            (
                "resend_full_access_api_key".to_owned(),
                json!("full-secret"),
            ),
            ("resend_from_email".to_owned(), json!("brain@example.com")),
            (
                "resend_webhook_signing_secret".to_owned(),
                json!("signing-secret"),
            ),
        ]),
        true,
    );
    fixture.write_users(
        r#"{
          "schema_version": 1,
          "users": [{
            "id": "pablo",
            "name": "Pablo",
            "phones": [],
            "emails": [{"value": "pablo@example.com", "inbound_allowed": true}],
            "response_email": "pablo@example.com"
          }]
        }"#,
    );

    let health = requirements(&fixture.command).expect("inspect selected workspace");

    assert_eq!(
        feature_status(&health, &RequirementScope::Receiver),
        FeatureStatus::Ready
    );
    assert_eq!(
        feature_status(&health, &RequirementScope::Email),
        FeatureStatus::Ready
    );
    assert_eq!(
        feature_status(&health, &RequirementScope::Sms),
        FeatureStatus::Off
    );
}

#[test]
fn disabling_receiver_removes_partial_channel_errors() {
    let fixture = Fixture::with_receiver(
        Map::from_iter([(
            "twilio_auth_token".to_owned(),
            Value::String("partial-secret".to_owned()),
        )]),
        false,
    );

    let health = requirements(&fixture.command).expect("inspect selected workspace");

    for scope in [
        RequirementScope::Receiver,
        RequirementScope::Sms,
        RequirementScope::Email,
    ] {
        assert_eq!(
            feature_status(&health, &scope),
            FeatureStatus::Off,
            "{scope:?}"
        );
    }
}
