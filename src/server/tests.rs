use super::{
    ReceiverFailureLog, habits_done_path, habits_url, provider_http_status, receiver_failure_log,
    session_done_path, url, verified_unavailable_email_response,
};

const FAMILY_ID: &str = "e806258e-491a-436d-9db4-a5ca9903e0d4";

#[test]
fn url_builds_a_localhost_route() {
    assert_eq!(url(8787, "/habits"), "http://127.0.0.1:8787/habits");
}

#[test]
fn url_always_includes_the_path() {
    assert!(url(8787, "/habits").ends_with("/habits"));
    assert!(url(1, "/habits").contains("/habits"));
}

#[test]
fn workspace_urls_carry_the_stable_opaque_ingress() {
    let ingress = crate::server::IngressId::parse(FAMILY_ID).unwrap();

    assert_eq!(
        habits_url(
            8787,
            ingress,
            crate::server::lifecycle::LeaseId::parse("57b162df-983a-45c3-ac7e-bad94eb27a99")
                .unwrap()
        ),
        format!(
            "http://127.0.0.1:8787/local/57b162df-983a-45c3-ac7e-bad94eb27a99/w/{FAMILY_ID}/habits"
        )
    );
    assert_eq!(
        habits_done_path(
            ingress,
            crate::server::lifecycle::LeaseId::parse("57b162df-983a-45c3-ac7e-bad94eb27a99")
                .unwrap()
        ),
        format!("/local/57b162df-983a-45c3-ac7e-bad94eb27a99/w/{FAMILY_ID}/habits/done")
    );
    assert_eq!(
        session_done_path(
            ingress,
            crate::server::lifecycle::LeaseId::parse("57b162df-983a-45c3-ac7e-bad94eb27a99")
                .unwrap()
        ),
        format!("/local/57b162df-983a-45c3-ac7e-bad94eb27a99/w/{FAMILY_ID}/session/done")
    );
}

#[test]
fn ignored_provider_events_are_logged_as_accepted_without_enqueue() {
    assert_eq!(
        receiver_failure_log(202, false),
        ReceiverFailureLog::AcceptedWithoutEnqueue
    );
    assert_eq!(
        receiver_failure_log(403, false),
        ReceiverFailureLog::Rejected
    );
    assert_eq!(
        receiver_failure_log(503, true),
        ReceiverFailureLog::Unavailable
    );
}

#[test]
fn resend_discard_outcomes_acknowledge_provider_success() {
    assert_eq!(
        provider_http_status(202, false, crate::server::receiver::Channel::Email),
        200
    );
    assert_eq!(
        provider_http_status(400, false, crate::server::receiver::Channel::Email),
        200
    );
    assert_eq!(
        provider_http_status(503, true, crate::server::receiver::Channel::Email),
        200
    );
    assert_eq!(
        provider_http_status(403, false, crate::server::receiver::Channel::Email),
        200
    );
    assert_eq!(
        provider_http_status(401, false, crate::server::receiver::Channel::Email),
        401
    );
    assert_eq!(
        provider_http_status(500, false, crate::server::receiver::Channel::Email),
        500
    );
    assert_eq!(
        provider_http_status(502, false, crate::server::receiver::Channel::Email),
        502
    );
}

#[test]
fn in_flight_verified_unavailable_email_retries_until_discard_is_ready() {
    let mut retry = Vec::new();
    verified_unavailable_email_response(false)
        .write_to(&mut retry)
        .expect("write retry response");
    let mut acknowledge = Vec::new();
    verified_unavailable_email_response(true)
        .write_to(&mut acknowledge)
        .expect("write acknowledged response");

    assert!(retry.starts_with(b"HTTP/1.1 503 Service Unavailable"));
    assert!(acknowledge.starts_with(b"HTTP/1.1 200 OK"));
}
