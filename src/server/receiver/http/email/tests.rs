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
    let body =
        br#"{"type":"email.received","data":{"from":"member@example.test","email_id":"email-1"}}"#;
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
        twilio_from_number: String::new(),
        public_base_url: String::new(),
        resend_signing_secret: format!("whsec_{}", STANDARD.encode(key)),
        resend_api_key: "selected-api-key".to_owned(),
        resend_from_email: "brain@example.test".to_owned(),
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

    assert_eq!(fetched.body, "Hello from email");
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
            Ok(
                br#"{"text":"hello","from":"member@example.test","attachments":[{"id":"one"}]}"#
                    .to_vec(),
            )
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

#[test]
fn a_real_from_header_with_a_display_name_still_authenticates() {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .to_string();
    let body = br#"{"type":"email.received","data":{"from":"Pablo Sarmiento <member@example.test>","email_id":"email-1"}}"#;
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
        twilio_from_number: String::new(),
        public_base_url: String::new(),
        resend_signing_secret: format!("whsec_{}", STANDARD.encode(key)),
        resend_api_key: "selected-api-key".to_owned(),
        resend_from_email: "brain@example.test".to_owned(),
    };

    let inbound = authenticate_payload(&headers, body, &config, |_, _| {
        Ok(FetchedEmail {
            body: "private prompt".to_owned(),
            sender: "Pablo Sarmiento <Member@Example.TEST>".to_owned(),
            participants: vec![
                "Pablo Sarmiento <Member@Example.TEST>".to_owned(),
                "\"Copy, A.\" <copy@example.test>".to_owned(),
                "not-an-address".to_owned(),
            ],
            attachments: Vec::new(),
            subject: "Original topic".to_owned(),
            message_id: None,
        })
    })
    .unwrap();

    assert_eq!(inbound.sender, "member@example.test");
    assert_eq!(
        inbound.participants,
        ["member@example.test", "copy@example.test"],
        "thread participants must reduce to bare addresses so the reply \
         allowlist can match them, dropping anything unparseable"
    );
}

#[test]
fn an_html_only_email_reaches_the_agent_as_text_within_the_prompt_budget() {
    let huge = "<p>Paragraph.</p>".repeat(4000);
    let email = format!(
        r#"{{"text":null,"html":{},"from":"sender@example.com","subject":"HTML only"}}"#,
        serde_json::Value::String(huge)
    );

    let fetched = parse_received_email(email.as_bytes(), br#"{"data":[]}"#).unwrap();

    assert!(
        fetched.body.starts_with("Paragraph."),
        "markup must not reach the agent: {}",
        &fetched.body[..40.min(fetched.body.len())]
    );
    assert!(!fetched.body.contains("<p>"));
    assert!(
        fetched.body.len() <= super::body::MAX_PROMPT_BYTES + 120,
        "an oversized email must be bounded before it is typed into the panel"
    );
    assert!(fetched.body.contains("truncated"));
}

#[test]
fn a_plain_text_email_is_still_delivered_verbatim() {
    let email = br#"{"text":"Ship the report by Friday.","html":"<p>ignored</p>","from":"sender@example.com"}"#;

    let fetched = parse_received_email(email, br#"{"data":[]}"#).unwrap();

    assert_eq!(fetched.body, "Ship the report by Friday.");
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
