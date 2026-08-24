use super::*;

#[test]
fn disabled_sms_target_returns_one_xml_unavailable_and_enqueues_nothing() {
    let mut fixture = SharedReceiverFixture::start_with_anchor();
    fixture.disable_target();

    let response = fixture.post_sms("SM-disabled-target", "discard disabled");
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert_eq!(response.matches("Brain is unavailable").count(), 1);
    assert!(response.contains("Content-Type: application/xml"));
    fixture.shutdown();
}

#[test]
fn persisted_disable_rejects_and_enqueues_nothing_before_control_refresh() {
    let mut fixture = SharedReceiverFixture::start_with_anchor();
    fixture.persist_target_disabled();

    let response = fixture.post_sms("SM-persisted-disable", "must not enqueue");
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert_eq!(response.matches("Brain is unavailable").count(), 1);
    fixture.shutdown();
}

#[test]
fn persisted_disable_verifies_and_remembers_resend_before_failed_live_refresh() {
    let mut fixture = SharedReceiverFixture::start_with_anchor();
    fixture.persist_target_disabled();

    let unavailable = fixture.post_received_email_event();
    fixture.persist_target_enabled();
    let replay = fixture.post_received_email_event();
    assert!(unavailable.starts_with("HTTP/1.1 200"), "{unavailable}");
    assert!(replay.starts_with("HTTP/1.1 200"), "{replay}");
    assert!(
        !replay.contains("Resend"),
        "replay fetched provider: {replay}"
    );
    assert!(!fixture.server_log().contains("Resend"));
    fixture.shutdown();
}

#[test]
fn invalid_resend_signature_during_persisted_disable_never_poisons_dedup() {
    let mut fixture = SharedReceiverFixture::start_with_anchor();
    fixture.persist_target_disabled();

    let invalid = fixture.post_email_without_credentials();
    fixture.persist_target_enabled();
    let valid = fixture.post_received_email_event();

    assert!(invalid.starts_with("HTTP/1.1 401"), "{invalid}");
    assert!(
        !valid.contains("duplicate provider delivery"),
        "invalid signature poisoned dedup: {valid}"
    );
    fixture.shutdown();
}
