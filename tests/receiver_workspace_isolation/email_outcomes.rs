use super::receiver_workspace_support::{SharedReceiverFixture, durable_jobs, provider_request};

fn assert_rejection(response: &str, code: &str) {
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let (_, body) = response.split_once("\r\n\r\n").unwrap();
    let body: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(body["ok"], false);
    assert_eq!(body["queued"], false);
    assert_eq!(body["error_code"], code);
    assert!(!body["error"].as_str().unwrap().is_empty());
}

#[test]
fn unrelated_recipient_is_acknowledged_without_disclosing_configured_addresses() {
    let mut fixture = SharedReceiverFixture::start();
    let response = provider_request::post(
        fixture.port,
        &provider_request::signed_received_email_event(
            "inbox@another.example.test",
            b"personal-resend-secret",
            "other-app-webhook",
            "other-app-email",
        ),
    );
    assert_rejection(&response, "unknown_recipient");
    assert!(!response.contains(provider_request::PERSONAL_EMAIL));
    assert!(durable_jobs(&fixture.workspace).is_empty());
    assert!(fixture.server_log().contains("code=unknown_recipient"));
    fixture.shutdown();
}

#[test]
fn permanent_email_rejections_return_codes_in_json_and_the_shared_log() {
    let mut fixture = SharedReceiverFixture::start();
    for (response, code) in [
        (fixture.post_ignored_email_event(), "ignored_event"),
        (fixture.post_permanent_email_event(), "invalid_request"),
    ] {
        assert_rejection(&response, code);
        assert!(fixture.server_log().contains(&format!("code={code}")));
    }
    assert!(durable_jobs(&fixture.workspace).is_empty());
    fixture.shutdown();
}

#[test]
fn verified_unavailable_email_returns_a_coded_acknowledgement() {
    let mut fixture = SharedReceiverFixture::start_with_anchor();
    fixture.unregister_target();
    assert_rejection(&fixture.post_received_email_event(), "receiver_unavailable");
    assert!(fixture.server_log().contains("code=receiver_unavailable"));
    assert!(durable_jobs(&fixture.workspace).is_empty());
    fixture.shutdown();
}

#[test]
fn invalid_signature_remains_an_authentication_failure_with_a_code() {
    let mut fixture = SharedReceiverFixture::start();
    let response = fixture.post_email_without_credentials();
    assert!(response.starts_with("HTTP/1.1 401"), "{response}");
    let (_, body) = response.split_once("\r\n\r\n").unwrap();
    let body: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(body["error_code"], "invalid_signature");
    assert!(durable_jobs(&fixture.workspace).is_empty());
    fixture.shutdown();
}

#[test]
fn recipient_domain_policy_filters_even_published_addresses_and_reloads_each_request() {
    let mut fixture = SharedReceiverFixture::start();
    for (pattern, recipient, code) in [
        (
            "*.example.test",
            "brain@personal.example.test",
            "ignored_event",
        ),
        (
            "*.example.test",
            "brain@deep.personal.example.test",
            "ignored_event",
        ),
        (
            "*.example.test",
            "brain@example.test",
            "recipient_domain_not_allowed",
        ),
        (
            "*.example.test",
            "brain@notexample.test",
            "recipient_domain_not_allowed",
        ),
        (
            "*.example.test",
            "brain@personal.example.test.invalid",
            "recipient_domain_not_allowed",
        ),
        (
            " *.EXAMPLE.TEST ",
            "Brain <BRAIN@PERSONAL.EXAMPLE.TEST>",
            "ignored_event",
        ),
        (
            "*.example.test",
            "brain@bad..example.test",
            "recipient_domain_not_allowed",
        ),
        ("example.test", "brain@example.test", "ignored_event"),
        (
            "example.test",
            "brain@personal.example.test",
            "recipient_domain_not_allowed",
        ),
        ("", "brain@another.test", "ignored_event"),
    ] {
        fixture.configure_email_policy(serde_json::json!(pattern), recipient);
        let response = provider_request::post(
            fixture.port,
            &provider_request::signed_email_event(
                recipient,
                b"personal-resend-secret",
                "domain-policy-webhook",
                "email.delivered",
            ),
        );
        assert_rejection(&response, code);
    }
    assert!(durable_jobs(&fixture.workspace).is_empty());
    assert!(
        fixture
            .server_log()
            .contains("code=recipient_domain_not_allowed")
    );
    fixture.shutdown();
}

#[test]
fn invalid_recipient_domain_configuration_remains_a_real_failure() {
    let mut fixture = SharedReceiverFixture::start();
    for policy in [
        serde_json::json!("*example.test"),
        serde_json::json!(["*.example.test"]),
    ] {
        fixture.configure_email_policy(policy, provider_request::PERSONAL_EMAIL);
        let response = fixture.post_ignored_email_event();
        assert!(response.starts_with("HTTP/1.1 500"), "{response}");
        assert!(durable_jobs(&fixture.workspace).is_empty());
    }
    fixture.shutdown();
}

#[test]
fn missing_receiving_credentials_are_retryable_instead_of_discarding_email() {
    let mut fixture = SharedReceiverFixture::start();
    let response = fixture.post_received_email_event();
    assert!(response.starts_with("HTTP/1.1 503"), "{response}");
    assert!(durable_jobs(&fixture.workspace).is_empty());
    fixture.shutdown();
}

#[test]
fn an_allowed_cc_cannot_authorize_a_workspace_on_a_forbidden_domain() {
    let mut fixture = SharedReceiverFixture::start();
    fixture.configure_email_policy(serde_json::json!("*.example.test"), "brain@outside.test");
    let response = provider_request::post(
        fixture.port,
        &provider_request::signed_email_payload(
            b"personal-resend-secret",
            "mixed-recipients",
            serde_json::json!({
                "type": "email.delivered",
                "data": {"to": ["brain@outside.test"], "cc": ["other@allowed.example.test"]},
            })
            .to_string(),
        ),
    );
    assert_rejection(&response, "unknown_recipient");
    assert!(durable_jobs(&fixture.workspace).is_empty());
    fixture.shutdown();
}

#[test]
fn an_allowed_workspace_in_cc_still_routes_when_to_belongs_to_another_app() {
    let mut fixture = SharedReceiverFixture::start();
    fixture.configure_email_policy(
        serde_json::json!("*.example.test"),
        provider_request::PERSONAL_EMAIL,
    );
    let response = provider_request::post(
        fixture.port,
        &provider_request::signed_email_payload(
            b"personal-resend-secret",
            "allowed-cc",
            serde_json::json!({
                "type": "email.delivered",
                "data": {"to": ["other@outside.test"], "cc": [provider_request::PERSONAL_EMAIL]},
            })
            .to_string(),
        ),
    );
    assert_rejection(&response, "ignored_event");
    assert!(durable_jobs(&fixture.workspace).is_empty());
    fixture.shutdown();
}
