mod body;

use serde::Deserialize;

use super::{AuthenticatedInbound, ProviderConfig, ProviderError};
use crate::server::receiver::{AttachmentRef, Channel};

#[derive(Deserialize)]
struct ResendWebhook {
    #[serde(rename = "type")]
    event_type: String,
    data: ResendData,
}

#[derive(Deserialize)]
struct ResendData {
    #[serde(default)]
    from: String,
    #[serde(default)]
    email_id: Option<String>,
}

struct FetchedEmail {
    body: String,
    sender: String,
    participants: Vec<String>,
    attachments: Vec<AttachmentRef>,
    subject: String,
    message_id: Option<String>,
}

struct EmailHeaders<'a> {
    webhook_id: &'a str,
    timestamp: &'a str,
    signature: &'a str,
}

pub(super) struct VerifiedEmail {
    webhook_id: String,
    email_id: String,
}

impl VerifiedEmail {
    pub(super) fn webhook_id(&self) -> &str {
        &self.webhook_id
    }
}

/// Every address a Resend webhook says the message reached, from the unverified
/// payload.
///
/// `to` and `cc` both count: brain answers on the workspace's own address
/// wherever the sender put it.
pub(super) fn destinations(body: &[u8]) -> Vec<String> {
    let Ok(payload) = serde_json::from_slice::<serde_json::Value>(body) else {
        return Vec::new();
    };
    let Some(data) = payload.get("data") else {
        return Vec::new();
    };
    ["to", "cc"]
        .into_iter()
        .flat_map(|field| addresses(data, field))
        .collect()
}

pub(super) fn verify(
    request: &crate::server::http::Request,
    body: &[u8],
    config: &ProviderConfig,
) -> Result<VerifiedEmail, ProviderError> {
    let headers = EmailHeaders {
        webhook_id: request.header("svix-id").unwrap_or_default(),
        timestamp: request.header("svix-timestamp").unwrap_or_default(),
        signature: request.header("svix-signature").unwrap_or_default(),
    };
    verify_payload(&headers, body, config)
}

pub(super) fn fetch(
    verified: VerifiedEmail,
    config: &ProviderConfig,
) -> Result<AuthenticatedInbound, ProviderError> {
    fetch_verified(verified, config, fetch_resend_email)
}

#[cfg(test)]
fn authenticate_payload(
    headers: &EmailHeaders<'_>,
    body: &[u8],
    config: &ProviderConfig,
    fetch: impl FnOnce(&str, &str) -> Result<FetchedEmail, ProviderError>,
) -> Result<AuthenticatedInbound, ProviderError> {
    let verified = verify_payload(headers, body, config)?;
    fetch_verified(verified, config, fetch)
}

fn verify_payload(
    headers: &EmailHeaders<'_>,
    body: &[u8],
    config: &ProviderConfig,
) -> Result<VerifiedEmail, ProviderError> {
    if config.resend_signing_secret.is_empty() {
        return Err(ProviderError::NotConfigured(
            "Resend security is not configured",
        ));
    }
    let authenticated = crate::server::security::verify_resend(
        &config.resend_signing_secret,
        headers.webhook_id,
        headers.timestamp,
        body,
        headers.signature,
    );
    if !authenticated {
        return Err(ProviderError::InvalidSignature("invalid Resend signature"));
    }
    let webhook: ResendWebhook = serde_json::from_slice(body)
        .map_err(|_| ProviderError::InvalidRequest("invalid Resend webhook JSON"))?;
    if webhook.event_type != "email.received" {
        return Err(ProviderError::IgnoredEvent);
    }
    if webhook.data.from.trim().is_empty() {
        return Err(ProviderError::InvalidRequest("email sender is missing"));
    }
    let Some(email_id) = webhook.data.email_id.as_deref() else {
        return Err(ProviderError::InvalidRequest(
            "received email has no email id",
        ));
    };
    Ok(VerifiedEmail {
        webhook_id: headers.webhook_id.to_owned(),
        email_id: email_id.to_owned(),
    })
}

fn fetch_verified(
    verified: VerifiedEmail,
    config: &ProviderConfig,
    fetch: impl FnOnce(&str, &str) -> Result<FetchedEmail, ProviderError>,
) -> Result<AuthenticatedInbound, ProviderError> {
    let fetched = fetch(&verified.email_id, &config.resend_full_access_api_key)?;
    if fetched.body.trim().is_empty() && fetched.attachments.is_empty() {
        return Err(ProviderError::InvalidRequest(
            "received email has no text body or attachment",
        ));
    }
    let sender = crate::users::normalize_mailbox(&fetched.sender)
        .map_err(|_| ProviderError::SenderNotAllowed("email sender is not allowed"))?;
    // Mail headers carry RFC 5322 mailboxes, not bare addresses. Everything
    // downstream — actor resolution and the reply allowlist — compares these
    // against configured identities, so an unreduced `Display Name <addr>`
    // would reject the sender and silently strip the thread of every
    // recipient. Unparseable participants are dropped, never carried forward.
    let participants = fetched
        .participants
        .iter()
        .filter_map(|value| crate::users::normalize_mailbox(value).ok())
        .collect();
    Ok(AuthenticatedInbound {
        channel: Channel::Email,
        sender,
        prompt: fetched.body,
        participants,
        attachments: fetched.attachments,
        receiving_address: config.resend_from_email.clone(),
        provider_id: Some(verified.webhook_id),
        email_reply: Some(crate::server::receiver::EmailReplyContext {
            provider_email_id: verified.email_id,
            subject: fetched.subject,
            message_id: fetched.message_id,
        }),
    })
}

pub(super) fn email_participants(data: &serde_json::Value) -> Vec<String> {
    let mut participants = Vec::new();
    if let Some(from) = data.get("from").and_then(serde_json::Value::as_str) {
        participants.push(from.to_owned());
    }
    for field in ["to", "cc", "reply_to"] {
        participants.extend(addresses(data, field));
    }
    participants
}

/// One address field of a Resend payload, which is a string or a list.
fn addresses(data: &serde_json::Value, field: &str) -> Vec<String> {
    match data.get(field) {
        Some(serde_json::Value::String(value)) => vec![value.clone()],
        Some(serde_json::Value::Array(values)) => values
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

mod fetch;

use fetch::fetch_resend_email;
pub(in crate::server::receiver) use fetch::refresh_attachment_access;
#[cfg(test)]
use fetch::{
    fetch_resend_email_with, parse_received_email, received_attachments_url, received_email_url,
    refresh_attachment_access_with, resend_status_hint,
};

#[cfg(test)]
mod tests;
