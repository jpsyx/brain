use std::fmt::{Display, Formatter};

use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

use super::ReceiverResponseKind;
use super::decode::{decode_email_envelope, decode_sms_envelope};

/// Byte-stable, acceptance-authorized provider payload.
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "channel", rename_all = "kebab-case")]
pub enum ReceiverDeliveryEnvelope {
    Sms { value: ReceiverSmsEnvelope },
    Email { value: ReceiverEmailEnvelope },
}

impl<'de> Deserialize<'de> for ReceiverDeliveryEnvelope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let Some(object) = value.as_object() else {
            return Err(D::Error::custom(
                "receiver delivery envelope shape is invalid",
            ));
        };
        if object.len() != 2 || !object.contains_key("channel") || !object.contains_key("value") {
            return Err(D::Error::custom(
                "receiver delivery envelope shape is invalid",
            ));
        }
        let channel = object.get("channel").and_then(serde_json::Value::as_str);
        let payload = object
            .get("value")
            .cloned()
            .ok_or_else(|| D::Error::custom("receiver delivery envelope shape is invalid"))?;
        match channel {
            Some("sms") => decode_sms_envelope(payload)
                .map(|value| Self::Sms { value })
                .map_err(D::Error::custom),
            Some("email") => decode_email_envelope(payload)
                .map(|value| Self::Email { value })
                .map_err(D::Error::custom),
            _ => Err(D::Error::custom(
                "receiver delivery envelope channel is invalid",
            )),
        }
    }
}

impl ReceiverDeliveryEnvelope {
    #[must_use]
    pub const fn sms(&self) -> Option<&ReceiverSmsEnvelope> {
        match self {
            Self::Sms { value } => Some(value),
            Self::Email { .. } => None,
        }
    }

    #[must_use]
    pub const fn email(&self) -> Option<&ReceiverEmailEnvelope> {
        match self {
            Self::Email { value } => Some(value),
            Self::Sms { .. } => None,
        }
    }
}

impl std::fmt::Debug for ReceiverDeliveryEnvelope {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiverDeliveryEnvelope(<redacted>)")
    }
}

/// Frozen SMS sender, destination, and rendered body.
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct ReceiverSmsEnvelope {
    pub(super) sender: String,
    pub(super) recipient: String,
    pub(super) body: String,
    pub(super) long_form_available: bool,
}

impl<'de> Deserialize<'de> for ReceiverSmsEnvelope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        decode_sms_envelope(value).map_err(D::Error::custom)
    }
}

impl ReceiverSmsEnvelope {
    #[must_use]
    pub fn sender(&self) -> &str {
        &self.sender
    }

    #[must_use]
    pub fn recipient(&self) -> &str {
        &self.recipient
    }

    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }

    #[must_use]
    pub const fn long_form_available(&self) -> bool {
        self.long_form_available
    }
}

impl std::fmt::Debug for ReceiverSmsEnvelope {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiverSmsEnvelope(<redacted>)")
    }
}

/// Frozen email sender, destinations, body alternatives, and provider lineage.
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct ReceiverEmailEnvelope {
    pub(super) sender: String,
    pub(super) recipients: Vec<String>,
    pub(super) subject: String,
    pub(super) text: String,
    pub(super) html: String,
    pub(super) in_reply_to: Option<String>,
    pub(super) references: Option<String>,
    pub(super) provider_email_id: Option<String>,
}

impl<'de> Deserialize<'de> for ReceiverEmailEnvelope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        decode_email_envelope(value).map_err(D::Error::custom)
    }
}

impl ReceiverEmailEnvelope {
    #[must_use]
    pub fn sender(&self) -> &str {
        &self.sender
    }

    #[must_use]
    pub fn recipients(&self) -> &[String] {
        &self.recipients
    }

    #[must_use]
    pub fn subject(&self) -> &str {
        &self.subject
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub fn html(&self) -> &str {
        &self.html
    }

    #[must_use]
    pub fn in_reply_to(&self) -> Option<&str> {
        self.in_reply_to.as_deref()
    }

    #[must_use]
    pub fn references(&self) -> Option<&str> {
        self.references.as_deref()
    }

    #[must_use]
    pub fn provider_email_id(&self) -> Option<&str> {
        self.provider_email_id.as_deref()
    }
}

impl std::fmt::Debug for ReceiverEmailEnvelope {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiverEmailEnvelope(<redacted>)")
    }
}

/// Failure to freeze one accepted response destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverDeliveryRenderError {
    InvalidOutboundSender,
    NoTrustedEmailRecipients,
    InvalidAcceptedEmailRecipient,
    InvalidAcceptedEmailProviderId,
    InvalidAcceptedEmailMessageId,
    InvalidAcceptedSmsRecipient,
}

impl Display for ReceiverDeliveryRenderError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidOutboundSender => "receiver delivery has an invalid outbound sender",
            Self::NoTrustedEmailRecipients => "receiver delivery has no trusted email recipients",
            Self::InvalidAcceptedEmailRecipient => {
                "receiver delivery has an invalid accepted email recipient"
            }
            Self::InvalidAcceptedEmailProviderId => {
                "receiver delivery has an invalid accepted email provider ID"
            }
            Self::InvalidAcceptedEmailMessageId => {
                "receiver delivery has an invalid accepted email message ID"
            }
            Self::InvalidAcceptedSmsRecipient => {
                "receiver delivery has an invalid accepted SMS recipient"
            }
        })
    }
}

impl std::error::Error for ReceiverDeliveryRenderError {}

/// Freeze the existing channel renderer and acceptance-time response identity.
///
/// # Errors
///
/// Returns [`ReceiverDeliveryRenderError::NoTrustedEmailRecipients`] when an
/// accepted email job contains no authorized destination,
/// [`ReceiverDeliveryRenderError::InvalidAcceptedEmailRecipient`] or
/// [`ReceiverDeliveryRenderError::InvalidAcceptedSmsRecipient`] when an
/// acceptance-time destination is invalid, and
/// [`ReceiverDeliveryRenderError::InvalidAcceptedEmailProviderId`] or
/// [`ReceiverDeliveryRenderError::InvalidAcceptedEmailMessageId`] when email
/// lineage is blank.
pub fn render_receiver_delivery(
    inbound: &crate::server::receiver::InboundJob,
    _response_kind: ReceiverResponseKind,
    outbound_sender: &str,
    content: &str,
) -> Result<ReceiverDeliveryEnvelope, ReceiverDeliveryRenderError> {
    match inbound.channel {
        crate::server::receiver::Channel::Sms => {
            let sender = crate::users::normalize_phone(outbound_sender)
                .ok()
                .filter(|normalized| normalized == outbound_sender)
                .ok_or(ReceiverDeliveryRenderError::InvalidOutboundSender)?;
            let recipient = crate::users::normalize_phone(&inbound.authenticated_sender)
                .ok()
                .filter(|normalized| normalized == &inbound.authenticated_sender)
                .ok_or(ReceiverDeliveryRenderError::InvalidAcceptedSmsRecipient)?;
            let reply = crate::server::reply::sms(content);
            Ok(ReceiverDeliveryEnvelope::Sms {
                value: ReceiverSmsEnvelope {
                    sender,
                    recipient,
                    body: reply.text,
                    long_form_available: reply.long_form_available,
                },
            })
        }
        crate::server::receiver::Channel::Email => {
            let sender = crate::users::normalize_mailbox(outbound_sender)
                .map_err(|_| ReceiverDeliveryRenderError::InvalidOutboundSender)?;
            if sender != outbound_sender {
                return Err(ReceiverDeliveryRenderError::InvalidOutboundSender);
            }
            let recipients = inbound
                .response_email
                .iter()
                .chain(&inbound.allowed_response_recipients)
                .map(|address| {
                    crate::users::normalize_mailbox(address)
                        .map_err(|_| ReceiverDeliveryRenderError::InvalidAcceptedEmailRecipient)
                })
                .collect::<Result<std::collections::BTreeSet<_>, _>>()?
                .into_iter()
                .collect::<Vec<_>>();
            if recipients.is_empty() {
                return Err(ReceiverDeliveryRenderError::NoTrustedEmailRecipients);
            }
            let reply = crate::server::reply::email(content);
            let lineage = inbound.email_reply.as_ref();
            let provider_email_id = lineage
                .map(|context| {
                    if context.provider_email_id.trim().is_empty() {
                        Err(ReceiverDeliveryRenderError::InvalidAcceptedEmailProviderId)
                    } else {
                        Ok(context.provider_email_id.clone())
                    }
                })
                .transpose()?;
            let message_id = lineage
                .and_then(|context| context.message_id.as_ref())
                .map(|message_id| {
                    if message_id.trim().is_empty() {
                        Err(ReceiverDeliveryRenderError::InvalidAcceptedEmailMessageId)
                    } else {
                        Ok(message_id.clone())
                    }
                })
                .transpose()?;
            Ok(ReceiverDeliveryEnvelope::Email {
                value: ReceiverEmailEnvelope {
                    sender,
                    recipients,
                    subject: crate::server::delivery::reply_subject(lineage),
                    html: crate::server::reply::email_html(&reply.text),
                    text: reply.text,
                    in_reply_to: message_id.clone(),
                    references: message_id,
                    provider_email_id,
                },
            })
        }
    }
}
