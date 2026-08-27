use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use super::ReceiverResponseKind;

/// Byte-stable, acceptance-authorized provider payload.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "channel", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ReceiverDeliveryEnvelope {
    Sms { value: ReceiverSmsEnvelope },
    Email { value: ReceiverEmailEnvelope },
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

/// Frozen SMS destination and rendered body.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiverSmsEnvelope {
    recipient: String,
    body: String,
    long_form_available: bool,
}

impl ReceiverSmsEnvelope {
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

/// Frozen email destinations, body alternatives, and provider lineage.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiverEmailEnvelope {
    recipients: Vec<String>,
    subject: String,
    text: String,
    html: String,
    in_reply_to: Option<String>,
    references: Option<String>,
    provider_email_id: Option<String>,
}

impl ReceiverEmailEnvelope {
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
    NoTrustedEmailRecipients,
}

impl Display for ReceiverDeliveryRenderError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("receiver delivery has no trusted email recipients")
    }
}

impl std::error::Error for ReceiverDeliveryRenderError {}

/// Freeze the existing channel renderer and acceptance-time response identity.
///
/// # Errors
///
/// Returns [`ReceiverDeliveryRenderError::NoTrustedEmailRecipients`] when an
/// accepted email job contains no authorized destination.
pub fn render_receiver_delivery(
    inbound: &crate::server::receiver::InboundJob,
    _response_kind: ReceiverResponseKind,
    content: &str,
) -> Result<ReceiverDeliveryEnvelope, ReceiverDeliveryRenderError> {
    match inbound.channel {
        crate::server::receiver::Channel::Sms => {
            let reply = crate::server::reply::sms(content);
            Ok(ReceiverDeliveryEnvelope::Sms {
                value: ReceiverSmsEnvelope {
                    recipient: inbound.authenticated_sender.clone(),
                    body: reply.text,
                    long_form_available: reply.long_form_available,
                },
            })
        }
        crate::server::receiver::Channel::Email => {
            let recipients = crate::server::delivery::trusted_response_recipients(
                inbound.response_email.as_deref(),
                &inbound.allowed_response_recipients,
            );
            if recipients.is_empty() {
                return Err(ReceiverDeliveryRenderError::NoTrustedEmailRecipients);
            }
            let reply = crate::server::reply::email(content);
            let lineage = inbound.email_reply.as_ref();
            let message_id = lineage.and_then(|context| context.message_id.clone());
            Ok(ReceiverDeliveryEnvelope::Email {
                value: ReceiverEmailEnvelope {
                    recipients,
                    subject: crate::server::delivery::reply_subject(lineage),
                    html: crate::server::reply::email_html(&reply.text),
                    text: reply.text,
                    in_reply_to: message_id.clone(),
                    references: message_id,
                    provider_email_id: lineage.map(|context| context.provider_email_id.clone()),
                },
            })
        }
    }
}
