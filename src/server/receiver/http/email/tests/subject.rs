use super::super::super::{AuthenticatedInbound, ProviderConfig, ProviderError};
use super::super::{VerifiedEmail, fetch_verified, parse_received_email};
use serde_json::{Value, json};

fn receive(email: &Value) -> Result<AuthenticatedInbound, ProviderError> {
    let config = ProviderConfig {
        workspace_id: crate::workspace::WorkspaceId::new(),
        twilio_auth_token: String::new(),
        twilio_from_number: String::new(),
        public_base_url: String::new(),
        resend_signing_secret: String::new(),
        resend_full_access_api_key: "full-access-key".to_owned(),
        resend_from_email: "brain@example.test".to_owned(),
    };
    let verified = VerifiedEmail {
        webhook_id: "webhook-subject".to_owned(),
        email_id: "email-subject".to_owned(),
    };
    fetch_verified(verified, &config, |_, _| {
        parse_received_email(&serde_json::to_vec(email).unwrap(), br#"{"data":[]}"#)
    })
}

#[test]
fn subject_only_email_is_accepted_as_the_prompt() {
    for parts in [
        json!({}),
        json!({"text": null, "html": null}),
        json!({"text": "", "html": ""}),
        json!({"text": " \r\n\t", "html": "<div><br>&nbsp;</div>"}),
    ] {
        let mut email = parts;
        email["from"] = json!("member@example.test");
        email["subject"] = json!("When is the next meeting?");
        email["message_id"] = json!("<message-subject@example.test>");
        email["attachments"] = json!([]);

        let inbound = receive(&email).expect("subject-only email must be accepted");

        assert!(
            inbound.prompt == "When is the next meeting?",
            "subject was not used as the prompt"
        );
        let reply = inbound.email_reply.unwrap();
        assert!(
            reply.subject == "When is the next meeting?",
            "reply subject was not preserved"
        );
        assert!(
            reply.message_id.as_deref() == Some("<message-subject@example.test>"),
            "reply message lineage was not preserved"
        );
    }
}

#[test]
fn email_body_takes_precedence_over_the_subject() {
    for (text, html, expected) in [
        ("Body question", "<p>HTML question</p>", "Body question"),
        (" \r\n", "<p>HTML question</p>", "HTML question"),
    ] {
        let inbound = receive(&json!({
            "from": "member@example.test",
            "subject": "Subject question",
            "text": text,
            "html": html,
        }))
        .unwrap();

        assert!(inbound.prompt == expected, "email body lost precedence");
    }
}

#[test]
fn subject_prompt_is_bounded_without_truncating_reply_metadata() {
    let subject = "次".repeat(6000);
    let inbound = receive(&json!({
        "from": "member@example.test",
        "subject": subject,
    }))
    .unwrap();

    let (prefix, notice) = inbound.prompt.split_once("\n\n").unwrap();
    assert!(
        prefix.chars().count() == 5461,
        "subject prompt was not bounded at a UTF-8 boundary"
    );
    assert!(prefix.chars().all(|character| character == '次'));
    assert!(notice.contains("truncated"));
    assert!(
        inbound.email_reply.unwrap().subject == subject,
        "reply subject was truncated"
    );
}
