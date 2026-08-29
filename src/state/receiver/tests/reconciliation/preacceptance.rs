#[test]
fn reconciliation_requeues_an_unaccepted_live_owner_before_later_fifo_work() {
    let fixture = launched_run("live-owner-timeout", 400_000);
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let later = receiver_job(Some("later-after-timeout"), 200);
    let later_acceptance = fixture
        .db
        .accept_receiver_job(&later, &identity)
        .expect("accept later receiver job");
    assert!(
        fixture
            .db
            .claim_next_receiver_run("later-instance", 91_200, 121_200)
            .expect("ordinary FIFO stays blocked")
            .is_none()
    );

    let effect = fixture
        .db
        .reconcile_next_receiver_job(91_200)
        .expect("reconcile exact acceptance expiry")
        .expect("pre-acceptance retry effect");
    assert_eq!(
        effect.action(),
        ReceiverReconciliationAction::RequeuePreAcceptance
    );
    assert_eq!(
        effect.reason(),
        ReceiverReconciliationReason::PreAcceptanceTimeout
    );
    assert_eq!(effect.job_id(), fixture.job_id);
    assert_eq!(effect.cleanup_instance(), Some("launch-instance"));
    let requeued = fixture
        .db
        .receiver_job(fixture.job_id)
        .expect("load requeued job")
        .expect("requeued job");
    assert_eq!(requeued.state(), ReceiverJobState::Retrying);
    assert_eq!(requeued.retry_count(), 1);
    assert_eq!(requeued.retry_at_unix_ms(), Some(96_200));
    assert_eq!(requeued.retry_from_state(), Some(ReceiverJobState::Launching));
    assert_eq!(requeued.recovery_count(), 0);
    assert_eq!(requeued.attempt_kind(), ReceiverAttemptKind::Ordinary);
    assert_eq!(requeued.observation_instance(), None);
    assert_eq!(requeued.observation_session_id(), None);
    assert_eq!(requeued.observation_revision(), 0);
    assert_eq!(requeued.launch_expires_at_unix_ms(), None);
    assert_eq!(requeued.acceptance_expires_at_unix_ms(), None);
    let registration_count = fixture
        .db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM receiver_session_registrations",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("count released registration");
    assert_eq!(registration_count, 0);
    let later_claim = fixture
        .db
        .claim_next_receiver_run("later-instance", 91_200, 121_200)
        .expect("claim later FIFO work")
        .expect("later FIFO work");
    assert_eq!(later_claim.job().id(), later_acceptance.job_id());
}

#[test]
fn expired_claim_owner_does_not_replace_lifecycle_deadlines() {
    let fixture = launched_run("expired-owner-timeout", 2_000);
    let before = fixture
        .db
        .receiver_job(fixture.job_id)
        .expect("load launched job")
        .expect("launched job");
    assert!(
        fixture
            .db
            .reconcile_next_receiver_job(2_000)
            .expect("reconcile expired claim before lifecycle expiry")
            .is_none()
    );
    assert_eq!(
        fixture
            .db
            .receiver_job(fixture.job_id)
            .expect("reload waiting job")
            .expect("waiting job"),
        before
    );
    assert!(
        fixture
            .db
            .claim_next_receiver_run("replacement-owner", 2_000, 32_000)
            .expect("ordinary claim cannot bypass ambiguous launch")
            .is_none()
    );
    let effect = fixture
        .db
        .reconcile_next_receiver_job(91_200)
        .expect("reconcile lifecycle expiry")
        .expect("expired-owner retry effect");
    assert_eq!(
        effect.action(),
        ReceiverReconciliationAction::RequeuePreAcceptance
    );
}
