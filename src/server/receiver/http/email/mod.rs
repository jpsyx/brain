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

const RESEND_RESPONSE_LIMIT: usize = 1024 * 1024;

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
    let fetched = fetch(&verified.email_id, &config.resend_api_key)?;
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

fn email_participants(data: &serde_json::Value) -> Vec<String> {
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

fn fetch_resend_email(email_id: &str, key: &str) -> Result<FetchedEmail, ProviderError> {
    fetch_resend_email_with(email_id, key, |url, limit| {
        fetch_resend_json(key, url, limit)
    })
}

fn fetch_resend_email_with(
    email_id: &str,
    key: &str,
    mut fetch: impl FnMut(&str, usize) -> Result<Vec<u8>, ProviderError>,
) -> Result<FetchedEmail, ProviderError> {
    if key.trim().is_empty() {
        return Err(ProviderError::NotConfigured(
            "RESEND_API_KEY is not configured",
        ));
    }
    let email = fetch(&received_email_url(email_id), RESEND_RESPONSE_LIMIT)?;
    ensure_response_limit(&email)?;
    let email_value: serde_json::Value = serde_json::from_slice(&email)
        .map_err(|_| ProviderError::Upstream("Resend returned invalid received email content"))?;
    let has_attachments = email_value
        .get("attachments")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|attachments| !attachments.is_empty());
    let attachments = if has_attachments {
        let response = fetch(&received_attachments_url(email_id), RESEND_RESPONSE_LIMIT)?;
        ensure_response_limit(&response)?;
        response
    } else {
        br#"{"data":[]}"#.to_vec()
    };
    parse_received_email(&email, &attachments)
}

fn ensure_response_limit(response: &[u8]) -> Result<(), ProviderError> {
    if response.len() > RESEND_RESPONSE_LIMIT {
        return Err(ProviderError::Upstream(
            "Resend receiving API response is too large",
        ));
    }
    Ok(())
}

fn fetch_resend_json(key: &str, url: &str, limit: usize) -> Result<Vec<u8>, ProviderError> {
    let max_time = super::RESEND_FETCH_TIMEOUT_SECONDS.to_string();
    let output = crate::server::provider::CurlRequest::new()
        .flag("silent")
        .flag("show-error")
        .flag("fail")
        .flag("location")
        .option("connect-timeout", "5")
        .option("max-time", &max_time)
        .option("header", &format!("Authorization: Bearer {key}"))
        .option("url", url)
        .output_limited(limit)
        .map_err(|_| ProviderError::Upstream("fetching Resend receiving API failed"))?;
    if !output.status.success() {
        return Err(ProviderError::Upstream(
            "Resend receiving API rejected the request",
        ));
    }
    Ok(output.stdout)
}

pub(in crate::server::receiver) fn refresh_attachment_access(
    command: &crate::workspace::CommandContext,
    message: &crate::server::receiver::InboundJob,
) -> Result<Vec<AttachmentRef>, ProviderError> {
    let Some(reply) = &message.email_reply else {
        return Ok(message.attachments.clone());
    };
    if message.attachments.is_empty() {
        return Ok(Vec::new());
    }
    let key = crate::server::provider::get(command, "resend_api_key").ok_or(
        ProviderError::NotConfigured("RESEND_API_KEY is not configured"),
    )?;
    refresh_attachment_access_with(
        &reply.provider_email_id,
        &key,
        &message.attachments,
        |url, limit| fetch_resend_json(&key, url, limit),
    )
}

fn refresh_attachment_access_with(
    email_id: &str,
    key: &str,
    accepted: &[AttachmentRef],
    mut fetch: impl FnMut(&str, usize) -> Result<Vec<u8>, ProviderError>,
) -> Result<Vec<AttachmentRef>, ProviderError> {
    if key.trim().is_empty() {
        return Err(ProviderError::NotConfigured(
            "RESEND_API_KEY is not configured",
        ));
    }
    let response = fetch(&received_attachments_url(email_id), RESEND_RESPONSE_LIMIT)?;
    ensure_response_limit(&response)?;
    let refreshed = parse_attachment_list(&response)?;
    accepted
        .iter()
        .map(|attachment| {
            let provider_id = attachment
                .provider_id
                .as_deref()
                .ok_or(ProviderError::Upstream(
                    "accepted Resend attachment has no stable identifier",
                ))?;
            refreshed
                .iter()
                .find(|candidate| candidate.provider_id.as_deref() == Some(provider_id))
                .cloned()
                .ok_or(ProviderError::Upstream(
                    "accepted Resend attachment is no longer available",
                ))
        })
        .collect()
}

fn parse_attachment_list(attachments: &[u8]) -> Result<Vec<AttachmentRef>, ProviderError> {
    let attachment_list: serde_json::Value = serde_json::from_slice(attachments)
        .map_err(|_| ProviderError::Upstream("Resend returned invalid attachment content"))?;
    Ok(attachment_list
        .get("data")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let url = item.get("download_url")?.as_str()?.to_owned();
                    Some(AttachmentRef {
                        url,
                        provider_id: item
                            .get("id")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_owned),
                        content_type: item
                            .get("content_type")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_owned),
                        filename: item
                            .get("filename")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_owned),
                    })
                })
                .collect()
        })
        .unwrap_or_default())
}

fn parse_received_email(email: &[u8], attachments: &[u8]) -> Result<FetchedEmail, ProviderError> {
    let email: serde_json::Value = serde_json::from_slice(email)
        .map_err(|_| ProviderError::Upstream("Resend returned invalid received email content"))?;
    let text = email
        .get("text")
        .and_then(serde_json::Value::as_str)
        .filter(|text| !text.trim().is_empty());
    let body = text.map_or_else(
        || {
            body::html_to_text(
                email
                    .get("html")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default(),
            )
        },
        str::to_owned,
    );
    let body = body::bounded_prompt(&body, body::MAX_PROMPT_BYTES);
    let sender = email
        .get("from")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let subject = email
        .get("subject")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let message_id = email
        .get("message_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let participants = email_participants(&email);
    let attachments = parse_attachment_list(attachments)?;
    Ok(FetchedEmail {
        body,
        sender,
        participants,
        attachments,
        subject,
        message_id,
    })
}

fn received_email_url(email_id: &str) -> String {
    format!("https://api.resend.com/emails/receiving/{email_id}")
}

fn received_attachments_url(email_id: &str) -> String {
    format!("https://api.resend.com/emails/receiving/{email_id}/attachments")
}

#[cfg(test)]
mod tests;
