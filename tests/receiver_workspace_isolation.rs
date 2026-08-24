#[path = "receiver_workspace_isolation/complete_lifecycle.rs"]
mod complete_lifecycle;
#[path = "receiver_workspace_isolation/durable_ingress.rs"]
mod durable_ingress;
#[path = "receiver_workspace_isolation/persisted_disable.rs"]
mod persisted_disable;
mod receiver_workspace_support;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use brain::server::receiver::execute_pipeline;
use receiver_workspace_support::{
    DualWorkspaceReceiverFixture, RecordingPipeline, RevocationPipeline, SharedReceiverFixture,
    durable_conversation_count, durable_jobs, job, poll_until,
};

#[test]
fn pipeline_resolves_before_credentials_and_authenticates_before_actor() {
    let mut pipeline = RecordingPipeline::new("personal", "personal-member");

    let result = execute_pipeline(&mut pipeline).unwrap();

    assert_eq!(result, "personal-member");
    assert_eq!(
        pipeline.events,
        [
            "resolve",
            "credentials",
            "signature",
            "actor",
            "job",
            "authority",
            "forward"
        ]
    );
}

#[test]
fn revoked_authority_after_actor_resolution_never_reaches_handoff() {
    let actor_resolved = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let authority_valid = Arc::new(AtomicBool::new(true));
    let forwards = Arc::new(AtomicUsize::new(0));
    let mut pipeline = RevocationPipeline {
        actor_resolved: Arc::clone(&actor_resolved),
        release: Arc::clone(&release),
        authority_valid: Arc::clone(&authority_valid),
        forwards: Arc::clone(&forwards),
    };
    let worker = std::thread::spawn(move || execute_pipeline(&mut pipeline));

    actor_resolved.wait();
    authority_valid.store(false, Ordering::Release);
    release.wait();

    let error = worker
        .join()
        .unwrap()
        .expect_err("revoked authority must reject the job");
    assert!(error.to_string().contains("revoked"), "{error:#}");
    assert_eq!(forwards.load(Ordering::Acquire), 0);
}

#[test]
fn same_sender_resolves_independently_in_each_selected_workspace() {
    let personal =
        execute_pipeline(&mut RecordingPipeline::new("personal", "personal-member")).unwrap();
    let family = execute_pipeline(&mut RecordingPipeline::new("family", "family-member")).unwrap();

    assert_eq!(personal, "personal-member");
    assert_eq!(family, "family-member");
    assert_ne!(personal, family);
}

#[test]
fn one_shared_process_routes_the_same_sender_to_two_exact_durable_queues() {
    let mut fixture = DualWorkspaceReceiverFixture::start();

    let swapped = fixture.post_personal_signed_with_family_credentials();
    assert!(swapped.starts_with("HTTP/1.1 401"), "{swapped}");
    assert!(fixture.personal_jobs().is_empty());
    assert!(fixture.family_jobs().is_empty());

    let personal_response = fixture.post_personal_async("SM-personal", "personal prompt");
    let family_response = fixture.post_family_async("SM-family", "family prompt");
    let (personal_jobs, family_jobs) = fixture.poll_both_jobs();

    assert!(
        personal_response
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .starts_with("HTTP/1.1 200")
    );
    assert!(
        family_response
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .starts_with("HTTP/1.1 200")
    );
    assert_eq!(personal_jobs.len(), 1);
    assert_eq!(personal_jobs[0].workspace_id, fixture.personal.id());
    assert_eq!(personal_jobs[0].actor.user_id().as_str(), "personal-member");
    assert_eq!(personal_jobs[0].prompt, "personal prompt");
    assert_eq!(family_jobs.len(), 1);
    assert_eq!(family_jobs[0].workspace_id, fixture.family.id());
    assert_eq!(family_jobs[0].actor.user_id().as_str(), "family-member");
    assert_eq!(family_jobs[0].prompt, "family prompt");
    fixture.shutdown();
}

#[test]
fn the_addressed_workspaces_own_credential_is_the_one_that_must_verify() {
    // Both workspaces answer on the same `/sms` URL, so a request that names
    // family's number is checked against family's token. Holding personal's
    // token buys nothing, and nothing is enqueued anywhere.
    let mut fixture = DualWorkspaceReceiverFixture::start();

    let crossed = fixture.post_family_signed_with_personal_credentials();

    assert!(crossed.starts_with("HTTP/1.1 401"), "{crossed}");
    assert!(fixture.family_jobs().is_empty());
    assert!(fixture.personal_jobs().is_empty());
    fixture.shutdown();
}

#[test]
fn absent_shared_process_stays_absent_and_has_no_responder() {
    let temp = tempfile::tempdir().unwrap();
    let paths = brain::server::lifecycle::ServerPaths::from_directory(temp.path().join("server"));
    let client = brain::server::control::ServerClient::new(paths.clone());

    assert!(client.connect_existing().is_err());
    assert!(!paths.process_record().exists());
    assert!(!paths.control_socket().exists());
}

#[test]
fn signed_unknown_sender_is_rejected_without_enqueuing() {
    let mut fixture = SharedReceiverFixture::start();

    let response = fixture.post_sms_from(
        "SM-unknown-sender",
        "must not enter the queue",
        "+12125550999",
    );
    assert!(response.starts_with("HTTP/1.1 403"), "{response}");
    assert!(durable_jobs(&fixture.workspace).is_empty());
    fixture.shutdown();
}

#[test]
fn accepted_receiver_route_rejects_body_over_one_mib_before_authentication() {
    let mut fixture = SharedReceiverFixture::start();

    let response = fixture.post_oversized_sms();
    assert!(response.starts_with("HTTP/1.1 413"), "{response}");
    assert!(durable_jobs(&fixture.workspace).is_empty());
    fixture.shutdown();
}

#[test]
fn missing_email_target_returns_one_json_unavailable_and_enqueues_nothing() {
    let mut fixture = SharedReceiverFixture::start_with_anchor();
    fixture.unregister_target();

    let response = fixture.post_email_without_credentials();
    assert!(response.starts_with("HTTP/1.1 401"), "{response}");
    assert!(durable_jobs(&fixture.workspace).is_empty());
    fixture.shutdown();
}

#[test]
fn authenticated_non_received_email_event_returns_accepted_without_enqueue() {
    let mut fixture = SharedReceiverFixture::start();

    let response = fixture.post_ignored_email_event();
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(durable_jobs(&fixture.workspace).is_empty());
    fixture.shutdown();
}

#[test]
fn repeated_resend_discard_outcomes_ack_without_enqueue_or_retry() {
    let mut fixture = SharedReceiverFixture::start();
    for response in [
        fixture.post_permanent_email_event(),
        fixture.post_permanent_email_event(),
        fixture.post_ignored_email_event(),
        fixture.post_ignored_email_event(),
    ] {
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(!response.contains("queued\":true"), "{response}");
    }
    assert!(durable_jobs(&fixture.workspace).is_empty());
    fixture.shutdown();
    drop(fixture);

    let mut fixture = SharedReceiverFixture::start_with_anchor();
    fixture.unregister_target();
    for response in [
        fixture.post_unavailable_email_event(),
        fixture.post_unavailable_email_event(),
    ] {
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.matches("Brain is unavailable").count() <= 1);
        assert!(!response.contains("queued\":true"), "{response}");
    }
    assert!(durable_jobs(&fixture.workspace).is_empty());
    fixture.shutdown();
}

#[test]
fn signed_resend_event_unavailable_before_credentials_is_rejected_on_live_replay() {
    let mut fixture = SharedReceiverFixture::start_with_anchor();
    fixture.unregister_target();

    let unavailable = fixture.post_received_email_event();
    fixture.register_target();
    let replay = fixture.post_received_email_event();
    assert!(unavailable.starts_with("HTTP/1.1 200"), "{unavailable}");
    assert!(replay.starts_with("HTTP/1.1 200"), "{replay}");
    assert!(
        !replay.contains("Resend"),
        "replayed unavailable event reached provider fetch: {replay}"
    );
    assert!(durable_jobs(&fixture.workspace).is_empty());
    let log = fixture.server_log();
    assert!(
        !log.contains("Resend"),
        "replayed unavailable event reached provider fetch: {log}"
    );
    fixture.shutdown();
}
