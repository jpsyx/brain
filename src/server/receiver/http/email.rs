use serde::Deserialize;
use tiny_http::Request;

use super::SecurityConfig;
use crate::server::receiver::{Attachment, Channel, InboundMessage};

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
    attachments: Vec<Attachment>,
}

pub(super) fn parse_email(
    request: &Request,
    body: &[u8],
    security: &SecurityConfig,
) -> Result<InboundMessage, (u16, String)> {
    if security.allowed_email.is_empty() {
        return Err((503, "email receiving is not configured".to_owned()));
    }
    if security.resend_signing_secret.is_empty() {
        return Err((503, "Resend security is not configured".to_owned()));
    }
    let header = |name: &str| {
        request
            .headers()
            .iter()
            .find(|header| header.field.to_string().eq_ignore_ascii_case(name))
            .map(|header| header.value.as_str())
            .unwrap_or_default()
    };
    let webhook_id = header("svix-id");
    let timestamp = header("svix-timestamp");
    if !crate::server::security::verify_resend(
        &security.resend_signing_secret,
        webhook_id,
        timestamp,
        body,
        header("svix-signature"),
    ) {
        return Err((403, "invalid Resend signature".to_owned()));
    }
    let webhook: ResendWebhook = serde_json::from_slice(body)
        .map_err(|_| (400, "invalid Resend webhook JSON".to_owned()))?;
    if webhook.event_type != "email.received" {
        return Err((202, "event ignored".to_owned()));
    }
    if !crate::server::security::sender_allowed(&webhook.data.from, &security.allowed_email) {
        return Err((403, "email sender is not allowed".to_owned()));
    }
    let Some(email_id) = webhook.data.email_id.as_deref() else {
        return Err((400, "received email has no email id".to_owned()));
    };
    let fetched = fetch_resend_email(email_id, &security.resend_api_key)?;
    if !crate::server::security::sender_allowed(&fetched.sender, &security.allowed_email) {
        return Err((403, "email sender is not allowed".to_owned()));
    }
    if fetched.body.trim().is_empty() && fetched.attachments.is_empty() {
        return Err((
            400,
            "received email has no text body or attachment".to_owned(),
        ));
    }
    Ok(InboundMessage {
        channel: Channel::Email,
        body: fetched.body,
        sender: fetched.sender,
        participants: fetched.participants,
        provider_id: Some(webhook_id.to_owned()),
        attachments: fetched.attachments,
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

fn fetch_resend_email(email_id: &str, key: &str) -> Result<FetchedEmail, (u16, String)> {
    if key.trim().is_empty() {
        return Err((503, "RESEND_API_KEY is not configured".to_owned()));
    }
    let email = fetch_resend_json(key, &received_email_url(email_id))?;
    let email_value: serde_json::Value = serde_json::from_slice(&email).map_err(|_| {
        (
            502,
            "Resend returned invalid received email content".to_owned(),
        )
    })?;
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

fn fetch_resend_json(key: &str, url: &str) -> Result<Vec<u8>, (u16, String)> {
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
        .map_err(|error| (502, format!("fetching Resend receiving API: {error}")))?;
    if !output.status.success() {
        return Err((502, "Resend receiving API rejected the request".to_owned()));
    }
    Ok(output.stdout)
}

fn parse_received_email(email: &[u8], attachments: &[u8]) -> Result<FetchedEmail, (u16, String)> {
    let email: serde_json::Value = serde_json::from_slice(email).map_err(|_| {
        (
            502,
            "Resend returned invalid received email content".to_owned(),
        )
    })?;
    let attachment_list: serde_json::Value = serde_json::from_slice(attachments)
        .map_err(|_| (502, "Resend returned invalid attachment content".to_owned()))?;
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
                    Some(Attachment {
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
        email_participants, parse_received_email, received_attachments_url, received_email_url,
    };
    use serde_json::json;

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
}
