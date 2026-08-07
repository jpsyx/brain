use std::io::Read as _;
use std::time::Duration;

use brain::server::control::{ControlRequest, ControlResponse};

use super::support::*;

#[test]
fn unavailable_posts_reject_before_body_io_and_leave_control_responsive() {
    let server = ServerFixture::new(FAMILY_ID);
    server.disable_family_receiver();

    for (path, expected_status) in [
        (format!("/w/{UNKNOWN_ID}/habits/done"), "404"),
        (format!("/w/{}/sms", server.family_ingress), "200"),
    ] {
        let mut partial = partial_post(server.port, &path, 1_000_000);
        let mut response = String::new();
        partial
            .read_to_string(&mut response)
            .expect("unavailable route must respond without waiting for its body");
        assert!(
            response.starts_with(&format!("HTTP/1.1 {expected_status}")),
            "{path}: {response}"
        );

        assert!(matches!(
            server
                .client
                .request_with_timeout(&ControlRequest::Snapshot, Duration::from_millis(500))
                .expect("rejected body must not occupy the control loop"),
            ControlResponse::Snapshot(_)
        ));
    }
}

#[test]
fn accepted_local_post_rejects_an_oversized_body() {
    let server = ServerFixture::new(FAMILY_ID);
    let oversized = "x".repeat(16 * 1024 + 1);

    let response = server.post(
        &format!(
            "/local/{}/w/{}/habits/done",
            server.family_lease, server.family_ingress
        ),
        &oversized,
    );

    assert!(response.starts_with("HTTP/1.1 413"), "{response}");
}

#[test]
fn unknown_receiver_ingress_returns_plain_not_found_without_provider_acknowledgement() {
    let server = ServerFixture::new(FAMILY_ID);

    for channel in ["sms", "email"] {
        let response = server.post(&format!("/w/{UNKNOWN_ID}/{channel}"), "provider body");
        assert!(response.starts_with("HTTP/1.1 404"), "{response}");
        assert!(!response.contains("Received"), "{response}");
        assert!(!response.contains("queued"), "{response}");
    }
}
