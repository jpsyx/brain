use anyhow::Result;
use serde::Deserialize;

use super::{AuthenticatedInbound, ProviderConfig};
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
}

struct EmailHeaders<'a> {
    webhook_id: &'a str,
    timestamp: &'a str,
    signature: &'a str,
}

pub(super) fn authenticate(
    request: &crate::server::http::Request,
    body: &[u8],
    config: &ProviderConfig,
) -> Result<AuthenticatedInbound> {
    let headers = EmailHeaders {
        webhook_id: request.header("svix-id").unwrap_or_default(),
        timestamp: request.header("svix-timestamp").unwrap_or_default(),
        signature: request.header("svix-signature").unwrap_or_default(),
    };
    authenticate_payload(&headers, body, config, fetch_resend_email)
}

fn authenticate_payload(
    headers: &EmailHeaders<'_>,
    body: &[u8],
    config: &ProviderConfig,
    fetch: impl FnOnce(&str, &str) -> Result<FetchedEmail>,
) -> Result<AuthenticatedInbound> {
    anyhow::ensure!(
        !config.resend_signing_secret.is_empty(),
        "Resend security is not configured"
    );
    let authenticated = crate::server::security::verify_resend(
        &config.resend_signing_secret,
        headers.webhook_id,
        headers.timestamp,
        body,
        headers.signature,
    );
    anyhow::ensure!(authenticated, "invalid Resend signature");
    let webhook: ResendWebhook =
        serde_json::from_slice(body).map_err(|_| anyhow::anyhow!("invalid Resend webhook JSON"))?;
    anyhow::ensure!(webhook.event_type == "email.received", "event ignored");
    anyhow::ensure!(
        !webhook.data.from.trim().is_empty(),
        "email sender is missing"
    );
    let Some(email_id) = webhook.data.email_id.as_deref() else {
        anyhow::bail!("received email has no email id");
    };
    let fetched = fetch(email_id, &config.resend_api_key)?;
    anyhow::ensure!(
        !fetched.body.trim().is_empty() || !fetched.attachments.is_empty(),
        "received email has no text body or attachment"
    );
    let sender = crate::users::normalize_email(&fetched.sender)
        .map_err(|_| anyhow::anyhow!("email sender is not allowed"))?;
    Ok(AuthenticatedInbound {
        channel: Channel::Email,
        sender,
        prompt: fetched.body,
        participants: fetched.participants,
        attachments: fetched.attachments,
        receiving_address: config.resend_from_email.clone(),
        provider_id: Some(headers.webhook_id.to_owned()),
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

fn fetch_resend_email(email_id: &str, key: &str) -> Result<FetchedEmail> {
    anyhow::ensure!(!key.trim().is_empty(), "RESEND_API_KEY is not configured");
    let email = fetch_resend_json(key, &received_email_url(email_id))?;
    let email_value: serde_json::Value = serde_json::from_slice(&email)
        .map_err(|_| anyhow::anyhow!("Resend returned invalid received email content"))?;
    let has_attachments = email_value
        .get("attachments")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|attachments| !attachments.is_empty());
    let attachments = if has_attachments {
        fetch_resend_json(key, &received_attachments_url(email_id))?
    } else {
        br#"{"data":[]}"#.to_vec()
    };
    parse_received_email(&email, &attachments)
}

fn fetch_resend_json(key: &str, url: &str) -> Result<Vec<u8>> {
    let output = crate::server::provider::CurlRequest::new()
        .flag("silent")
        .flag("show-error")
        .flag("fail")
        .flag("location")
        .option("connect-timeout", "10")
        .option("max-time", "30")
        .option("header", &format!("Authorization: Bearer {key}"))
        .option("url", url)
        .output()
        .map_err(|error| anyhow::anyhow!("fetching Resend receiving API: {error}"))?;
    anyhow::ensure!(
        output.status.success(),
        "Resend receiving API rejected the request"
    );
    Ok(output.stdout)
}

fn parse_received_email(email: &[u8], attachments: &[u8]) -> Result<FetchedEmail> {
    let email: serde_json::Value = serde_json::from_slice(email)
        .map_err(|_| anyhow::anyhow!("Resend returned invalid received email content"))?;
    let attachment_list: serde_json::Value = serde_json::from_slice(attachments)
        .map_err(|_| anyhow::anyhow!("Resend returned invalid attachment content"))?;
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
    let participants = email_participants(&email);
    let attachments = attachment_list
        .get("data")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let url = item.get("download_url")?.as_str()?.to_owned();
                    Some(AttachmentRef {
                        url,
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
        .unwrap_or_default();
    Ok(FetchedEmail {
        body,
        sender,
        participants,
        attachments,
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
            "attachments": [{"id":"a1","filename":"paper.pdf","content_type":"application/pdf"}]
        }"#;
        let attachments = br#"{
            "data": [{
                "download_url": "https://inbound.example/paper",
                "filename": "paper.pdf",
                "content_type": "application/pdf"
            }]
        }"#;

        let fetched = parse_received_email(email, attachments).unwrap();

        assert_eq!(fetched.body, "<p>Hello from email</p>");
        assert_eq!(fetched.sender, "sender@example.com");
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
