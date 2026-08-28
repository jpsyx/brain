use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

use super::{
    ReceiverDeliveryEnvelope, ReceiverDeliveryRenderError, ReceiverEmailEnvelope,
    ReceiverProviderCapability, ReceiverSmsEnvelope,
};

const SAFE_NOTICE: &str =
    "I couldn’t deliver the full response on the original channel. Please try again there.";

/// One alternate destination whose authority was frozen with the inbound job.
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct ReceiverFallbackDestination {
    provider: ReceiverProviderCapability,
    sender: String,
    recipient: String,
}

impl ReceiverFallbackDestination {
    /// Freeze one authenticated SMS alternate.
    pub fn sms(
        sender: impl Into<String>,
        recipient: impl Into<String>,
    ) -> Result<Self, ReceiverDeliveryRenderError> {
        Self::validated(
            ReceiverProviderCapability::Twilio,
            sender.into(),
            recipient.into(),
        )
    }

    /// Freeze one authenticated email alternate.
    pub fn email(
        sender: impl Into<String>,
        recipient: impl Into<String>,
    ) -> Result<Self, ReceiverDeliveryRenderError> {
        Self::validated(
            ReceiverProviderCapability::Resend,
            sender.into(),
            recipient.into(),
        )
    }

    fn validated(
        provider: ReceiverProviderCapability,
        sender: String,
        recipient: String,
    ) -> Result<Self, ReceiverDeliveryRenderError> {
        match provider {
            ReceiverProviderCapability::Twilio => {
                let valid_sender = crate::users::normalize_phone(&sender)
                    .ok()
                    .is_some_and(|normalized| normalized == sender);
                let valid_recipient = crate::users::normalize_phone(&recipient)
                    .ok()
                    .is_some_and(|normalized| normalized == recipient);
                if !valid_sender {
                    return Err(ReceiverDeliveryRenderError::InvalidOutboundSender);
                }
                if !valid_recipient {
                    return Err(ReceiverDeliveryRenderError::InvalidAcceptedSmsRecipient);
                }
            }
            ReceiverProviderCapability::Resend => {
                crate::users::validate_canonical_mailbox(&sender)
                    .map_err(|_| ReceiverDeliveryRenderError::InvalidOutboundSender)?;
                crate::users::validate_canonical_mailbox(&recipient)
                    .map_err(|_| ReceiverDeliveryRenderError::InvalidAcceptedEmailRecipient)?;
            }
        }
        Ok(Self {
            provider,
            sender,
            recipient,
        })
    }

    #[must_use]
    pub const fn provider(&self) -> ReceiverProviderCapability {
        self.provider
    }

    #[must_use]
    pub fn recipient(&self) -> &str {
        &self.recipient
    }
}

impl<'de> Deserialize<'de> for ReceiverFallbackDestination {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawDestination {
            provider: ReceiverProviderCapability,
            sender: String,
            recipient: String,
        }

        let raw = RawDestination::deserialize(deserializer)?;
        Self::validated(raw.provider, raw.sender, raw.recipient)
            .map_err(|_| D::Error::custom("receiver fallback destination is invalid"))
    }
}

impl std::fmt::Debug for ReceiverFallbackDestination {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiverFallbackDestination(<redacted>)")
    }
}

/// At most one conservative notice on a frozen alternate channel.
#[derive(Clone, PartialEq, Eq)]
pub struct ReceiverFallbackPlan {
    destination: ReceiverFallbackDestination,
}

impl ReceiverFallbackPlan {
    #[must_use]
    pub const fn destination(&self) -> &ReceiverFallbackDestination {
        &self.destination
    }

    #[must_use]
    pub const fn notice(&self) -> &'static str {
        SAFE_NOTICE
    }
}

impl std::fmt::Debug for ReceiverFallbackPlan {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiverFallbackPlan(<redacted>)")
    }
}

/// Select only acceptance-frozen authority, never mutable users or configuration.
#[must_use]
pub fn plan_receiver_fallback(
    failed_provider: ReceiverProviderCapability,
    attempted_recipients: &[&str],
    frozen_alternates: &[ReceiverFallbackDestination],
) -> Option<ReceiverFallbackPlan> {
    frozen_alternates
        .iter()
        .find(|candidate| {
            candidate.provider != failed_provider
                && !attempted_recipients.contains(&candidate.recipient.as_str())
        })
        .cloned()
        .map(|destination| ReceiverFallbackPlan { destination })
}

pub(in crate::state::receiver) fn render_receiver_fallback(
    plan: &ReceiverFallbackPlan,
) -> ReceiverDeliveryEnvelope {
    match plan.destination.provider {
        ReceiverProviderCapability::Twilio => ReceiverDeliveryEnvelope::Sms {
            value: ReceiverSmsEnvelope {
                sender: plan.destination.sender.clone(),
                recipient: plan.destination.recipient.clone(),
                body: SAFE_NOTICE.to_owned(),
                long_form_available: false,
            },
        },
        ReceiverProviderCapability::Resend => ReceiverDeliveryEnvelope::Email {
            value: ReceiverEmailEnvelope {
                sender: plan.destination.sender.clone(),
                recipients: vec![plan.destination.recipient.clone()],
                subject: crate::server::delivery::reply_subject(None),
                text: SAFE_NOTICE.to_owned(),
                html: crate::server::reply::email_html(SAFE_NOTICE),
                in_reply_to: None,
                references: None,
                provider_email_id: None,
            },
        },
    }
}
