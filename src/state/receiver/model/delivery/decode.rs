use serde::Deserialize;

use super::{ReceiverEmailEnvelope, ReceiverSmsEnvelope};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawReceiverSmsEnvelope {
    sender: String,
    recipient: String,
    body: String,
    long_form_available: bool,
}

pub(super) fn decode_sms_envelope(
    value: serde_json::Value,
) -> Result<ReceiverSmsEnvelope, &'static str> {
    let raw = serde_json::from_value::<RawReceiverSmsEnvelope>(value)
        .map_err(|_| "receiver SMS delivery envelope is invalid")?;
    let sender = crate::users::normalize_phone(&raw.sender)
        .ok()
        .filter(|normalized| normalized == &raw.sender)
        .ok_or("receiver SMS delivery envelope is invalid")?;
    let recipient = crate::users::normalize_phone(&raw.recipient)
        .ok()
        .filter(|normalized| normalized == &raw.recipient)
        .ok_or("receiver SMS delivery envelope is invalid")?;
    if raw.body.chars().count() > crate::server::reply::SMS_LIMIT {
        return Err("receiver SMS delivery envelope is invalid");
    }
    Ok(ReceiverSmsEnvelope {
        sender,
        recipient,
        body: raw.body,
        long_form_available: raw.long_form_available,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawReceiverEmailEnvelope {
    sender: String,
    recipients: Vec<String>,
    subject: String,
    text: String,
    html: String,
    in_reply_to: Option<String>,
    references: Option<String>,
    provider_email_id: Option<String>,
}

pub(super) fn decode_email_envelope(
    value: serde_json::Value,
) -> Result<ReceiverEmailEnvelope, &'static str> {
    let raw = serde_json::from_value::<RawReceiverEmailEnvelope>(value)
        .map_err(|_| "receiver email delivery envelope is invalid")?;
    crate::users::validate_canonical_mailbox(&raw.sender)
        .map_err(|_| "receiver email delivery envelope is invalid")?;
    if raw.recipients.is_empty()
        || raw.subject.trim().is_empty()
        || raw.subject.trim() != raw.subject
        || raw.text.trim() != raw.text
        || raw.html.trim().is_empty()
        || raw
            .provider_email_id
            .as_ref()
            .is_some_and(|value| value.trim().is_empty())
    {
        return Err("receiver email delivery envelope is invalid");
    }
    let normalized = raw
        .recipients
        .iter()
        .map(|recipient| crate::users::normalize_email(recipient))
        .collect::<Result<std::collections::BTreeSet<_>, _>>()
        .map_err(|_| "receiver email delivery envelope is invalid")?
        .into_iter()
        .collect::<Vec<_>>();
    if normalized != raw.recipients {
        return Err("receiver email delivery envelope is invalid");
    }
    match (&raw.in_reply_to, &raw.references) {
        (None, None) => {}
        (Some(in_reply_to), Some(references))
            if !in_reply_to.trim().is_empty()
                && in_reply_to == references
                && raw.provider_email_id.is_some() => {}
        _ => return Err("receiver email delivery envelope is invalid"),
    }
    Ok(ReceiverEmailEnvelope {
        sender: raw.sender,
        recipients: raw.recipients,
        subject: raw.subject,
        text: raw.text,
        html: raw.html,
        in_reply_to: raw.in_reply_to,
        references: raw.references,
        provider_email_id: raw.provider_email_id,
    })
}
