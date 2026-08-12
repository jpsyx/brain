use super::{FeatureStatus, PromptMetadata};

/// Receiver, SMS, and email health for one workspace.
///
/// `machine_env` is the registry's machine-global map: the public receiver
/// origin lives there, because one machine serves one URL per channel for every
/// workspace registered on it.
pub(super) fn statuses(
    enabled: bool,
    env: &serde_json::Map<String, serde_json::Value>,
    machine_env: &serde_json::Map<String, serde_json::Value>,
    users: Option<&crate::users::Users>,
) -> (FeatureStatus, FeatureStatus, FeatureStatus) {
    if !enabled {
        return (FeatureStatus::Off, FeatureStatus::Off, FeatureStatus::Off);
    }
    let has_phone_mapping = users.is_some_and(|users| {
        users
            .users
            .iter()
            .any(|user| user.phones.iter().any(|identity| identity.inbound_allowed))
    });
    let has_email_mapping = users.is_some_and(|users| {
        users
            .users
            .iter()
            .any(|user| user.emails.iter().any(|identity| identity.inbound_allowed))
    });
    let sms_active = has_phone_mapping
        || any_present(
            env,
            &[
                "twilio_account_sid",
                "twilio_auth_token",
                "twilio_from_number",
            ],
        );
    let email_active = has_email_mapping
        || any_present(
            env,
            &[
                "resend_sending_api_key",
                "resend_full_access_api_key",
                "resend_from_email",
                "resend_webhook_signing_secret",
            ],
        );
    let has_public_url = field_present(machine_env, "brain_receiver_public_url");
    let sms = channel_status(
        sms_active,
        has_phone_mapping
            && has_public_url
            && all_present(
                env,
                &[
                    "twilio_account_sid",
                    "twilio_auth_token",
                    "twilio_from_number",
                ],
            ),
    );
    let email = channel_status(
        email_active,
        has_email_mapping
            && has_public_url
            && all_present(
                env,
                &[
                    "resend_sending_api_key",
                    "resend_full_access_api_key",
                    "resend_from_email",
                    "resend_webhook_signing_secret",
                ],
            ),
    );
    let receiver = if matches!(sms, FeatureStatus::Incomplete)
        || matches!(email, FeatureStatus::Incomplete)
        || matches!((sms, email), (FeatureStatus::Off, FeatureStatus::Off))
    {
        FeatureStatus::Incomplete
    } else {
        FeatureStatus::Ready
    };
    (receiver, sms, email)
}

const fn channel_status(active: bool, complete: bool) -> FeatureStatus {
    match (active, complete) {
        (false, _) => FeatureStatus::Off,
        (true, true) => FeatureStatus::Ready,
        (true, false) => FeatureStatus::Incomplete,
    }
}

fn any_present(env: &serde_json::Map<String, serde_json::Value>, names: &[&str]) -> bool {
    names.iter().any(|name| env.contains_key(*name))
}

fn all_present(env: &serde_json::Map<String, serde_json::Value>, names: &[&str]) -> bool {
    names.iter().all(|name| field_present(env, name))
}

fn field_present(env: &serde_json::Map<String, serde_json::Value>, name: &str) -> bool {
    env.get(name)
        .and_then(serde_json::Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
}

pub(super) fn sms_prompts() -> Vec<PromptMetadata> {
    vec![
        PromptMetadata::plain("Receiver public URL"),
        PromptMetadata::plain("Twilio account SID"),
        PromptMetadata::secret("Twilio auth token"),
        PromptMetadata::plain("Twilio from number"),
        PromptMetadata::plain("Allowed phone mapping"),
    ]
}

pub(super) fn email_prompts() -> Vec<PromptMetadata> {
    vec![
        PromptMetadata::plain("Receiver public URL"),
        PromptMetadata::secret("Resend API key"),
        PromptMetadata::plain("Resend from email"),
        PromptMetadata::secret("Resend webhook signing secret"),
        PromptMetadata::plain("Allowed email mapping"),
    ]
}
