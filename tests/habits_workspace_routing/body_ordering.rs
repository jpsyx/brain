use std::io::Read as _;
use std::time::Duration;

use brain::server::control::{ControlRequest, ControlResponse};

use super::support::*;

#[test]
fn an_unknown_local_post_rejects_before_body_io_and_leaves_control_responsive() {
    let server = ServerFixture::new(FAMILY_ID);
    server.disable_family_receiver();

    let path = format!("/w/{UNKNOWN_ID}/habits/done");
    let mut partial = partial_post(server.port, &path, 1_000_000);
    let mut response = String::new();
    partial
        .read_to_string(&mut response)
        .expect("unknown route must respond without waiting for its body");
    assert!(response.starts_with("HTTP/1.1 404"), "{path}: {response}");

    assert!(matches!(
        server
            .client
            .request_with_timeout(&ControlRequest::Snapshot, Duration::from_millis(500))
            .expect("rejected body must not occupy the control loop"),
        ControlResponse::Snapshot(_)
    ));
}

#[test]
fn a_provider_body_that_never_arrives_never_occupies_the_control_loop() {
    // One machine-wide `/sms` URL means the destination that selects a workspace
    // lives inside the body, so the boundary must read it before it can route or
    // decline. A body that never finishes must still leave control answering.
    let server = ServerFixture::new(FAMILY_ID);
    server.disable_family_receiver();

    let _outstanding = partial_post(server.port, "/sms", 1_000_000);

    assert!(matches!(
        server
            .client
            .request_with_timeout(&ControlRequest::Snapshot, Duration::from_millis(500))
            .expect("an outstanding provider body must not occupy the control loop"),
        ControlResponse::Snapshot(_)
    ));
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
fn a_retired_ingress_path_returns_plain_not_found_without_provider_acknowledgement() {
    let server = ServerFixture::new(FAMILY_ID);

    for channel in ["sms", "email"] {
        let response = server.post(&format!("/w/{UNKNOWN_ID}/{channel}"), "provider body");
        assert!(response.starts_with("HTTP/1.1 404"), "{response}");
        assert!(!response.contains("Received"), "{response}");
        assert!(!response.contains("queued"), "{response}");
    }
}

#[test]
fn an_address_no_workspace_publishes_returns_plain_not_found() {
    // The URL exists for every workspace on the machine, so "not found" is now a
    // statement about the destination inside the payload.
    let server = ServerFixture::new(FAMILY_ID);

    let response = server.post("/sms", "Body=hello&From=%2B12125550100&To=%2B19995550000");

    assert!(response.starts_with("HTTP/1.1 404"), "{response}");
    assert!(!response.contains("Received"), "{response}");
}
