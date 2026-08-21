use super::super::{ProviderError, RESEND_FETCH_TIMEOUT_SECONDS};
use super::{FetchedEmail, body, email_participants};
use crate::server::receiver::AttachmentRef;

const RESEND_RESPONSE_LIMIT: usize = 1024 * 1024;

pub(super) fn fetch_resend_email(email_id: &str, key: &str) -> Result<FetchedEmail, ProviderError> {
    fetch_resend_email_with(email_id, key, |url, limit| {
        fetch_resend_json(key, url, limit)
    })
}

pub(super) fn fetch_resend_email_with(
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
    let max_time = RESEND_FETCH_TIMEOUT_SECONDS.to_string();
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
        // The provider is told only that an upstream call failed. The owner
        // needs the status: 401/403 is a key that cannot read inbound mail,
        // 404 is a key that belongs to a different account than the address,
        // and a missing status is a network fault rather than a refusal.
        crate::logging::log(format!(
            "receiver email fetch rejected {} url={url}",
            upstream_status(key, url).map_or_else(
                || "with no HTTP status (could not reach Resend)".to_owned(),
                |status| format!("with HTTP {status}{}", resend_status_hint(status)),
            )
        ));
        return Err(ProviderError::Upstream(
            "Resend receiving API rejected the request",
        ));
    }
    Ok(output.stdout)
}

/// What a rejected receiving-API status usually means, for the local log.
pub(super) fn resend_status_hint(status: u16) -> &'static str {
    match status {
        401 | 403 => {
            " — resend_sending_api_key cannot read inbound mail; the key needs full access, not sending-only"
        }
        404 => " — this email id is not in the account resend_sending_api_key belongs to",
        429 => " — rate limited by Resend",
        _ => "",
    }
}

/// Re-ask for just the status of a refused fetch. The request is a GET, so
/// repeating it is safe, and it keeps the success path's parsing untouched.
fn upstream_status(key: &str, url: &str) -> Option<u16> {
    let max_time = RESEND_FETCH_TIMEOUT_SECONDS.to_string();
    let output = crate::server::provider::CurlRequest::new()
        .flag("silent")
        .option("output", "/dev/null")
        .option("write-out", "%{http_code}")
        .option("connect-timeout", "5")
        .option("max-time", &max_time)
        .option("header", &format!("Authorization: Bearer {key}"))
        .option("url", url)
        .output_limited(16)
        .ok()?;
    String::from_utf8(output.stdout).ok()?.trim().parse().ok()
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
    let key = crate::server::provider::get(command, "resend_full_access_api_key").ok_or(
        ProviderError::NotConfigured("resend_full_access_api_key is not configured"),
    )?;
    refresh_attachment_access_with(
        &reply.provider_email_id,
        &key,
        &message.attachments,
        |url, limit| fetch_resend_json(&key, url, limit),
    )
}

pub(super) fn refresh_attachment_access_with(
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

pub(super) fn parse_received_email(
    email: &[u8],
    attachments: &[u8],
) -> Result<FetchedEmail, ProviderError> {
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

pub(super) fn received_email_url(email_id: &str) -> String {
    format!("https://api.resend.com/emails/receiving/{email_id}")
}

pub(super) fn received_attachments_url(email_id: &str) -> String {
    format!("https://api.resend.com/emails/receiving/{email_id}/attachments")
}
