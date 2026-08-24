use super::*;

#[test]
fn signed_sms_commits_durable_job_before_provider_success() {
    let mut fixture = SharedReceiverFixture::start();
    let response_rx = fixture.post_sms_async("SM-task-five", "hello from shared HTTP");
    let mut legacy_queue = brain::tui::receiver::InboundQueue::default();
    poll_until(Instant::now() + Duration::from_secs(3), || {
        fixture
            .socket
            .poll_jobs(fixture.workspace.id(), &mut legacy_queue);
        let durable = durable_jobs(&fixture.workspace);
        if durable.is_empty()
            && let Ok(response) = response_rx.try_recv()
        {
            panic!("shared receiver responded before durable commit: {response}");
        }
        !durable.is_empty()
    });
    let response = response_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let durable = durable_jobs(&fixture.workspace);

    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert_eq!(durable.len(), 1);
    assert_eq!(durable[0].workspace_id, fixture.workspace.id());
    assert_eq!(durable[0].actor.user_id().as_str(), "personal-member");
    assert_eq!(durable[0].prompt, "hello from shared HTTP");
    assert!(
        legacy_queue.is_empty(),
        "ingress still dispatched work into the live TUI queue"
    );
    fixture.shutdown();
}

#[test]
fn response_loss_and_provider_retry_return_success_for_one_durable_job() {
    let mut fixture = SharedReceiverFixture::start();
    fixture.post_sms_without_response("SM-response-loss", "original committed prompt");
    poll_until(Instant::now() + Duration::from_secs(3), || {
        durable_jobs(&fixture.workspace).len() == 1
    });

    let retry = fixture.post_sms("SM-response-loss", "retry must not replace original");
    let durable = durable_jobs(&fixture.workspace);

    assert!(retry.starts_with("HTTP/1.1 200"), "{retry}");
    assert_eq!(durable.len(), 1);
    assert_eq!(durable[0].prompt, "original committed prompt");
    assert_eq!(durable_conversation_count(&fixture.workspace), 1);
    fixture.shutdown();
}

#[test]
fn response_loss_then_shared_process_crash_preserves_the_original_durable_job() {
    let mut fixture = SharedReceiverFixture::start();
    fixture.post_sms_without_response("SM-process-crash", "before process crash");
    poll_until(Instant::now() + Duration::from_secs(3), || {
        durable_jobs(&fixture.workspace).len() == 1
    });
    fixture.crash_and_recover_server();

    let retry = fixture.post_sms("SM-process-crash", "after process crash");
    let durable = durable_jobs(&fixture.workspace);

    assert!(retry.starts_with("HTTP/1.1 200"), "{retry}");
    assert_eq!(durable.len(), 1);
    assert_eq!(durable[0].prompt, "before process crash");
    assert_eq!(durable_conversation_count(&fixture.workspace), 1);
    fixture.shutdown();
}

#[test]
fn full_durable_queue_returns_sms_unavailable_without_a_sixty_fifth_job() {
    let mut fixture = SharedReceiverFixture::start();
    let db = brain::state::Db::open(&fixture.workspace).expect("durable receiver state");
    let identity = brain::state::ReceiverConversationIdentity::sms(
        fixture.workspace.id(),
        brain::users::UserId::parse("member").expect("seed user ID"),
    );
    for index in 0..64 {
        db.accept_receiver_job(
            &job(&fixture.workspace, &format!("seed {index}")),
            &identity,
        )
        .expect("seed queued capacity");
    }
    drop(db);

    let response_rx = fixture.post_sms_async("SM-capacity-overflow", "must remain unavailable");
    let mut legacy_queue = brain::tui::receiver::InboundQueue::default();
    let mut response = None;
    poll_until(Instant::now() + Duration::from_secs(3), || {
        fixture
            .socket
            .poll_jobs(fixture.workspace.id(), &mut legacy_queue);
        if let Ok(received) = response_rx.try_recv() {
            response = Some(received);
        }
        response.is_some()
    });
    let response = response.expect("capacity response");

    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert_eq!(response.matches("Brain is unavailable").count(), 1);
    assert!(response.contains("Content-Type: application/xml"));
    assert_eq!(durable_jobs(&fixture.workspace).len(), 64);
    assert_eq!(durable_conversation_count(&fixture.workspace), 1);
    assert!(legacy_queue.is_empty());
    fixture.shutdown();
}
