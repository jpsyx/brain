#[path = "receiver_workspace_isolation/complete_lifecycle.rs"]
mod complete_lifecycle;
#[path = "receiver_workspace_isolation/persisted_disable.rs"]
mod persisted_disable;
mod receiver_workspace_support;

use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use brain::server::receiver::{
    Channel, INBOUND_QUEUE_CAPACITY, execute_pipeline, forward_job, forward_or_unavailable,
};
use brain::tui::singleton::JobSocket;
use receiver_workspace_support::{
    DualWorkspaceReceiverFixture, FAMILY_ID, PERSONAL_ID, RecordingPipeline, RevocationPipeline,
    SharedReceiverFixture, job, poll_until, workspace,
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
fn one_shared_process_routes_the_same_sender_to_two_exact_workspace_sockets() {
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
fn failed_socket_is_discarded_with_one_unavailable_response_and_no_retry() {
    let temp = tempfile::tempdir().unwrap();
    let personal = workspace(&temp, PERSONAL_ID, "personal");
    let missing_socket = personal.paths().job_socket();

    let outcome = forward_or_unavailable(&missing_socket, &job(&personal, "discard me"));

    assert!(!outcome.forwarded);
    assert!(!outcome.retry_scheduled);
    assert_eq!(outcome.responses.len(), 1);
    assert_eq!(
        outcome.responses[0].body,
        "Brain is unavailable for this workspace. Please try again when its TUI is open."
    );
    assert!(!missing_socket.exists());
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
fn signed_sms_routes_through_shared_process_into_exact_live_tui() {
    let mut fixture = SharedReceiverFixture::start();
    let response_rx = fixture.post_sms_async("SM-task-five", "hello from shared HTTP");
    let mut queue = Vec::new();
    poll_until(Instant::now() + Duration::from_secs(3), || {
        fixture.socket.poll_jobs(fixture.workspace.id(), &mut queue);
        if queue.is_empty()
            && let Ok(response) = response_rx.try_recv()
        {
            panic!("shared receiver responded without enqueue: {response}");
        }
        !queue.is_empty()
    });
    let response = response_rx.recv_timeout(Duration::from_secs(2)).unwrap();

    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert_eq!(queue.len(), 1);
    assert_eq!(queue[0].workspace_id, fixture.workspace.id());
    assert_eq!(queue[0].actor.user_id().as_str(), "personal-member");
    assert_eq!(queue[0].prompt, "hello from shared HTTP");
    fixture.shutdown();
}

#[test]
fn signed_unknown_sender_is_rejected_without_enqueuing() {
    let mut fixture = SharedReceiverFixture::start();

    let response = fixture.post_sms_from(
        "SM-unknown-sender",
        "must not enter the queue",
        "+12125550999",
    );
    let mut queue = Vec::new();
    fixture.socket.poll_jobs(fixture.workspace.id(), &mut queue);

    assert!(response.starts_with("HTTP/1.1 403"), "{response}");
    assert!(queue.is_empty());
    fixture.shutdown();
}

#[test]
fn accepted_receiver_route_rejects_body_over_one_mib_before_authentication() {
    let mut fixture = SharedReceiverFixture::start();

    let response = fixture.post_oversized_sms();
    let mut queue = Vec::new();
    fixture.socket.poll_jobs(fixture.workspace.id(), &mut queue);

    assert!(response.starts_with("HTTP/1.1 413"), "{response}");
    assert!(queue.is_empty());
    fixture.shutdown();
}

#[test]
fn missing_email_target_returns_one_json_unavailable_and_enqueues_nothing() {
    let mut fixture = SharedReceiverFixture::start_with_anchor();
    fixture.unregister_target();

    let response = fixture.post_email_without_credentials();
    let mut queue = Vec::new();
    fixture.socket.poll_jobs(fixture.workspace.id(), &mut queue);

    assert!(response.starts_with("HTTP/1.1 401"), "{response}");
    assert!(queue.is_empty());
    fixture.shutdown();
}

#[test]
fn authenticated_non_received_email_event_returns_accepted_without_enqueue() {
    let mut fixture = SharedReceiverFixture::start();

    let response = fixture.post_ignored_email_event();
    let mut queue = Vec::new();
    fixture.socket.poll_jobs(fixture.workspace.id(), &mut queue);

    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(queue.is_empty());
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
    let mut queue = Vec::new();
    fixture.socket.poll_jobs(fixture.workspace.id(), &mut queue);
    assert!(queue.is_empty());
    fixture.shutdown();

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
    fixture.socket.poll_jobs(fixture.workspace.id(), &mut queue);
    assert!(queue.is_empty());
    fixture.shutdown();
}

#[test]
fn signed_resend_event_unavailable_before_credentials_is_rejected_on_live_replay() {
    let mut fixture = SharedReceiverFixture::start_with_anchor();
    fixture.unregister_target();

    let unavailable = fixture.post_received_email_event();
    fixture.register_target();
    let replay = fixture.post_received_email_event();
    let mut queue = Vec::new();
    fixture.socket.poll_jobs(fixture.workspace.id(), &mut queue);

    assert!(unavailable.starts_with("HTTP/1.1 200"), "{unavailable}");
    assert!(replay.starts_with("HTTP/1.1 200"), "{replay}");
    assert!(
        !replay.contains("Resend"),
        "replayed unavailable event reached provider fetch: {replay}"
    );
    assert!(queue.is_empty());
    let log = fixture.server_log();
    assert!(
        !log.contains("Resend"),
        "replayed unavailable event reached provider fetch: {log}"
    );
    fixture.shutdown();
}

#[test]
fn job_socket_acknowledges_only_the_matching_workspace_enqueue() {
    let temp = tempfile::tempdir().unwrap();
    let personal = workspace(&temp, PERSONAL_ID, "personal");
    let family = workspace(&temp, FAMILY_ID, "family");
    let socket = JobSocket::bind(&personal).unwrap();
    let personal_job = job(&personal, "hello personal");
    let path = personal.paths().job_socket();
    let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);

    std::thread::spawn(move || {
        result_tx.send(forward_job(&path, &personal_job)).unwrap();
    });

    let mut queue = Vec::new();
    poll_until(Instant::now() + Duration::from_secs(1), || {
        socket.poll_jobs(personal.id(), &mut queue);
        !queue.is_empty()
    });

    result_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert_eq!(queue.len(), 1);
    assert_eq!(queue[0].workspace_id, personal.id());
    assert_eq!(queue[0].prompt, "hello personal");
    assert_ne!(queue[0].workspace_id, family.id());
    assert!(queue.len() <= INBOUND_QUEUE_CAPACITY);

    let family_job = job(&family, "must stay in family");
    let personal_path = personal.paths().job_socket();
    let (rejected_tx, rejected_rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        rejected_tx
            .send(forward_job(&personal_path, &family_job))
            .unwrap();
    });
    poll_until(Instant::now() + Duration::from_secs(1), || {
        socket.poll_jobs(personal.id(), &mut queue);
        rejected_rx.try_recv().is_ok_and(|result| result.is_err())
    });
    assert_eq!(queue.len(), 1, "family work entered the personal queue");
}

#[test]
fn failed_ack_write_rolls_back_the_just_enqueued_job() {
    let temp = tempfile::tempdir().unwrap();
    let personal = workspace(&temp, PERSONAL_ID, "personal");
    let socket = JobSocket::bind(&personal).unwrap();
    let mut client = UnixStream::connect(personal.paths().job_socket()).unwrap();
    let staged = job(&personal, "must roll back");
    client
        .write_all(&serde_json::to_vec(&staged).unwrap())
        .unwrap();
    client.write_all(b"\n").unwrap();
    let workspace_id = personal.id();
    let (queue_tx, queue_rx) = std::sync::mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let mut queue = Vec::new();
        socket.poll_jobs(workspace_id, &mut queue);
        queue_tx.send(queue).unwrap();
    });
    let mut prepared = [0_u8; 9];
    client.read_exact(&mut prepared).unwrap();
    assert_eq!(&prepared, b"prepared\n");
    client.write_all(b"commit\n").unwrap();
    client.shutdown(std::net::Shutdown::Both).unwrap();
    drop(client);

    let queue = queue_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    worker.join().unwrap();

    assert!(queue.is_empty());
}

#[test]
fn full_tui_queue_rejects_and_returns_one_unavailable_response_without_retry() {
    let temp = tempfile::tempdir().unwrap();
    let personal = workspace(&temp, PERSONAL_ID, "personal");
    let socket = JobSocket::bind(&personal).unwrap();
    let mut queue = vec![job(&personal, "already queued"); INBOUND_QUEUE_CAPACITY];
    let path = personal.paths().job_socket();
    let rejected_job = job(&personal, "discard when full");
    let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);

    std::thread::spawn(move || {
        result_tx
            .send(forward_or_unavailable(&path, &rejected_job))
            .unwrap();
    });
    poll_until(Instant::now() + Duration::from_secs(1), || {
        socket.poll_jobs(personal.id(), &mut queue);
        result_rx.try_recv().is_ok_and(|outcome| {
            assert!(!outcome.forwarded);
            assert!(!outcome.retry_scheduled);
            assert_eq!(outcome.responses.len(), 1);
            assert_eq!(outcome.responses[0].channel, Channel::Sms);
            true
        })
    });

    assert_eq!(queue.len(), INBOUND_QUEUE_CAPACITY);
}
