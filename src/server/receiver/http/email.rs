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
    let sender = crate::users::normalize_email(&fetched.sender)
        .map_err(|_| ProviderError::SenderNotAllowed("email sender is not allowed"))?;
    Ok(AuthenticatedInbound {
        channel: Channel::Email,
        sender,
        prompt: fetched.body,
        participants: fetched.participants,
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
        match data.get(field) {
            Some(serde_json::Value::String(value)) => participants.push(value.clone()),
            Some(serde_json::Value::Array(values)) => participants.extend(
                values
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_owned),
            ),
            _ => {}
        }
    }
    participants
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
    let body = email
        .get("text")
        .and_then(serde_json::Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .or_else(|| email.get("html").and_then(serde_json::Value::as_str))
        .unwrap_or_default()
        .to_owned();
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
mod tests {
    use super::{
        EmailHeaders, FetchedEmail, authenticate_payload, email_participants, parse_received_email,
        received_attachments_url, received_email_url,
    };
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use hmac::{Hmac, Mac as _};
    use serde_json::json;
    use sha2::Sha256;

    #[test]
    fn authenticated_email_uses_injected_fetch_without_external_io() {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .to_string();
        let body = br#"{"type":"email.received","data":{"from":"member@example.test","email_id":"email-1"}}"#;
        let key = b"email-test-secret";
        let signature = resend_signature(key, "webhook-1", &timestamp, body);
        let headers = EmailHeaders {
            webhook_id: "webhook-1",
            timestamp: &timestamp,
            signature: &signature,
        };
        let config = super::ProviderConfig {
            workspace_id: crate::workspace::WorkspaceId::new(),
            twilio_auth_token: String::new(),
            public_base_url: String::new(),
            resend_signing_secret: format!("whsec_{}", STANDARD.encode(key)),
            resend_api_key: "selected-api-key".to_owned(),
            resend_from_email: "brain@example.test".to_owned(),
            ingress_id: crate::server::IngressId::new(),
        };
        let inbound = authenticate_payload(&headers, body, &config, |email_id, api_key| {
            assert_eq!(email_id, "email-1");
            assert_eq!(api_key, "selected-api-key");
            Ok(FetchedEmail {
                body: "private prompt".to_owned(),
                sender: "member@example.test".to_owned(),
                participants: vec!["member@example.test".to_owned()],
                attachments: Vec::new(),
                subject: "Original topic".to_owned(),
                message_id: Some("<message-1@example.test>".to_owned()),
            })
        })
        .unwrap();

        assert_eq!(inbound.sender, "member@example.test");
        assert_eq!(inbound.prompt, "private prompt");
        assert_eq!(inbound.receiving_address, "brain@example.test");
    }

    #[test]
    fn participants_include_from_to_cc_and_reply_to() {
        let data = json!({
            "from": "sender@example.com",
            "to": ["brain@example.com"],
            "cc": ["copy@example.com"],
            "reply_to": ["reply@example.com"]
        });
        assert_eq!(
            email_participants(&data),
            vec![
                "sender@example.com",
                "brain@example.com",
                "copy@example.com",
                "reply@example.com"
            ]
        );
    }

    #[test]
    fn received_email_uses_the_resend_receiving_endpoints() {
        assert_eq!(
            received_email_url("email-123"),
            "https://api.resend.com/emails/receiving/email-123"
        );
        assert_eq!(
            received_attachments_url("email-123"),
            "https://api.resend.com/emails/receiving/email-123/attachments"
        );
    }

    #[test]
    fn received_email_uses_html_fallback_participants_and_attachment_downloads() {
        let email = br#"{
            "text": null,
            "html": "<p>Hello from email</p>",
            "from": "sender@example.com",
            "to": ["brain@example.com"],
            "cc": ["copy@example.com"],
            "reply_to": ["reply@example.com"],
            "subject": "Original topic",
            "message_id": "<message-1@example.com>",
            "attachments": [{"id":"a1","filename":"paper.pdf","content_type":"application/pdf"}]
        }"#;
        let attachments = br#"{
            "data": [{
                "id": "a1",
                "download_url": "https://inbound.example/paper",
                "filename": "paper.pdf",
                "content_type": "application/pdf"
            }]
        }"#;

        let fetched = parse_received_email(email, attachments).unwrap();

        assert_eq!(fetched.body, "<p>Hello from email</p>");
        assert_eq!(fetched.sender, "sender@example.com");
        assert_eq!(fetched.subject, "Original topic");
        assert_eq!(
            fetched.message_id.as_deref(),
            Some("<message-1@example.com>")
        );
        assert_eq!(
            fetched.participants,
            vec![
                "sender@example.com",
                "brain@example.com",
                "copy@example.com",
                "reply@example.com"
            ]
        );
        assert_eq!(fetched.attachments.len(), 1);
        assert_eq!(fetched.attachments[0].provider_id.as_deref(), Some("a1"));
        assert_eq!(fetched.attachments[0].url, "https://inbound.example/paper");
        assert_eq!(
            fetched.attachments[0].content_type.as_deref(),
            Some("application/pdf")
        );
        assert_eq!(
            fetched.attachments[0].filename.as_deref(),
            Some("paper.pdf")
        );
    }

    #[test]
    fn email_reply_payload_preserves_subject_and_message_lineage() {
        let payload = crate::server::delivery::email_payload(
            "brain@example.test",
            &["member@example.test".to_owned()],
            "Re: Original topic",
            "text",
            "<p>text</p>",
            Some("<message-1@example.test>"),
        );

        assert_eq!(payload["subject"], "Re: Original topic");
        assert_eq!(
            payload["headers"]["In-Reply-To"],
            "<message-1@example.test>"
        );
        assert_eq!(payload["headers"]["References"], "<message-1@example.test>");
        assert_eq!(payload["to"], serde_json::json!(["member@example.test"]));
    }

    #[test]
    fn delayed_attachment_access_is_refreshed_from_stable_provider_ids() {
        let accepted = vec![crate::server::receiver::AttachmentRef {
            url: "https://expired.example/old-token".to_owned(),
            provider_id: Some("attachment-1".to_owned()),
            content_type: Some("application/pdf".to_owned()),
            filename: Some("paper.pdf".to_owned()),
        }];
        let refreshed = super::refresh_attachment_access_with(
            "email-1",
            "secret-key",
            &accepted,
            |url, limit| {
                assert_eq!(
                    url,
                    "https://api.resend.com/emails/receiving/email-1/attachments"
                );
                assert_eq!(limit, 1024 * 1024);
                Ok(br#"{"data":[{"id":"attachment-1","download_url":"https://fresh.example/new-token","filename":"paper.pdf","content_type":"application/pdf"}]}"#.to_vec())
            },
        )
        .unwrap();

        assert_eq!(refreshed.len(), 1);
        assert_eq!(refreshed[0].url, "https://fresh.example/new-token");
        assert_eq!(refreshed[0].provider_id.as_deref(), Some("attachment-1"));
        assert!(!format!("{refreshed:?}").contains("secret-key"));
    }

    #[test]
    fn invalid_receiving_api_json_is_a_typed_upstream_failure() {
        let Err(error) = parse_received_email(b"not-json", br#"{"data":[]}"#) else {
            panic!("invalid provider JSON must fail");
        };

        assert_eq!(error.status(), 502);
        assert!(!error.unavailable());
    }

    #[test]
    fn oversized_received_email_is_rejected_at_the_injected_fetch_boundary() {
        let mut calls = Vec::new();
        let result = super::fetch_resend_email_with("email-oversized", "key", |url, limit| {
            calls.push((url.to_owned(), limit));
            Ok(vec![b'x'; limit + 1])
        });
        let Err(error) = result else {
            panic!("oversized provider body must stop before JSON parsing");
        };

        assert_eq!(error.status(), 502);
        assert_eq!(
            calls,
            [(received_email_url("email-oversized"), 1024 * 1024)]
        );
    }

    #[test]
    fn oversized_attachment_metadata_is_rejected_at_the_same_fetch_boundary() {
        let mut calls = Vec::new();
        let result = super::fetch_resend_email_with("email-attachment", "key", |url, limit| {
            calls.push((url.to_owned(), limit));
            if url.ends_with("/attachments") {
                Ok(vec![b'x'; limit + 1])
            } else {
                Ok(br#"{"text":"hello","from":"member@example.test","attachments":[{"id":"one"}]}"#.to_vec())
            }
        });
        let Err(error) = result else {
            panic!("oversized attachment response must stop before JSON parsing");
        };

        assert_eq!(error.status(), 502);
        assert_eq!(
            calls,
            [
                (received_email_url("email-attachment"), 1024 * 1024),
                (received_attachments_url("email-attachment"), 1024 * 1024)
            ]
        );
    }

    fn resend_signature(key: &[u8], id: &str, timestamp: &str, body: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(key).unwrap();
        mac.update(id.as_bytes());
        mac.update(b".");
        mac.update(timestamp.as_bytes());
        mac.update(b".");
        mac.update(body);
        format!("v1,{}", STANDARD.encode(mac.finalize().into_bytes()))
    }
}
