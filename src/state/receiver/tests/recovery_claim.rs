struct StalledRunFixture {
    db: Db,
    inbound: crate::server::receiver::InboundJob,
    job_id: ReceiverJobId,
    ordinary: ReceiverJob,
}

fn stalled_run(provider_id: &str) -> StalledRunFixture {
    let db = Db::open_in_memory().expect("receiver state");
    let inbound = receiver_job(Some(provider_id), 100);
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let accepted = db
        .accept_receiver_job(&inbound, &identity)
        .expect("accept receiver job");
    let job_id = accepted.job_id();
    db.claim_next_receiver_run("ordinary-owner", 1_000, 2_000)
        .expect("claim ordinary run")
        .expect("ordinary run");
    assert!(
        db.prepare_receiver_job_launch(job_id, "ordinary-owner", 1_100)
            .expect("prepare ordinary launch")
    );
    register_observation_session(
        &db,
        accepted.conversation_id(),
        &inbound,
        "ordinary-instance",
        "native-session",
    );
    let token = db
        .receiver_job(job_id)
        .expect("load ordinary job")
        .expect("ordinary job")
        .token();
    assert!(
        db.commit_receiver_job_launch(
            job_id,
            "ordinary-owner",
            &launch_observation(token, "ordinary-instance", "native-session", 1_200),
        )
        .expect("commit ordinary launch")
    );
    assert!(
        db.apply_receiver_observation(
            job_id,
            "ordinary-owner",
            &observation(
                token,
                "ordinary-instance",
                "native-session",
                ReceiverNonterminalObservationPhase::Accepted,
                1,
                1_300,
            ),
        )
        .expect("commit ordinary acceptance")
    );
    assert!(
        db.apply_receiver_observation(
            job_id,
            "ordinary-owner",
            &observation(
                token,
                "ordinary-instance",
                "native-session",
                ReceiverNonterminalObservationPhase::Progressing,
                2,
                1_400,
            ),
        )
        .expect("commit ordinary progress")
    );
    let ordinary = db
        .receiver_job(job_id)
        .expect("load stalled ordinary job")
        .expect("stalled ordinary job");
    StalledRunFixture {
        db,
        inbound,
        job_id,
        ordinary,
    }
}

#[test]
fn recovery_claim_preserves_identity_resets_the_cursor_and_consumes_only_recovery_budget() {
    let fixture = stalled_run("durable-recovery-claim");

    let claimed = fixture
        .db
        .claim_receiver_recovery_run(fixture.job_id, "recovery-owner", 301_400, 331_400)
        .expect("claim recovery run")
        .expect("recovery run");
    let recovery = claimed.job();

    assert_eq!(claimed.claim().owner(), "recovery-owner");
    assert_eq!(claimed.claim().expires_at_unix_ms(), 331_400);
    assert_eq!(recovery.state(), ReceiverJobState::Claimed);
    assert_eq!(recovery.id(), fixture.ordinary.id());
    assert_eq!(recovery.token(), fixture.ordinary.token());
    assert_eq!(recovery.conversation_id(), fixture.ordinary.conversation_id());
    assert_eq!(recovery.inbound(), &fixture.inbound);
    assert_eq!(
        recovery.accepted_at_unix_ms(),
        fixture.ordinary.accepted_at_unix_ms()
    );
    assert_eq!(
        recovery.progressing_at_unix_ms(),
        fixture.ordinary.progressing_at_unix_ms()
    );
    assert_eq!(recovery.retry_count(), fixture.ordinary.retry_count());
    assert_eq!(recovery.recovery_count(), 1);
    assert_eq!(recovery.attempt_kind(), ReceiverAttemptKind::Recovery);
    assert_eq!(recovery.observation_instance(), None);
    assert_eq!(recovery.observation_session_id(), None);
    assert_eq!(recovery.observation_revision(), 0);
    assert_eq!(recovery.attempt_accepted_at_unix_ms(), None);
    assert_eq!(recovery.attempt_progressing_at_unix_ms(), None);
    assert_eq!(recovery.latest_progress_at_unix_ms(), None);
    assert_eq!(
        recovery
            .observation_cursor()
            .expect("fresh recovery cursor"),
        crate::agent::AgentObservationCursor::launched()
    );
    assert_eq!(recovery.launch_expires_at_unix_ms(), Some(421_400));
    assert_eq!(recovery.recovery_expires_at_unix_ms(), Some(601_400));
    assert_eq!(recovery.acceptance_expires_at_unix_ms(), None);
    assert_eq!(recovery.progress_expires_at_unix_ms(), None);
    assert_eq!(
        recovery.absolute_work_expires_at_unix_ms(),
        fixture.ordinary.absolute_work_expires_at_unix_ms()
    );
}

#[test]
fn recovery_claim_cas_failure_leaves_the_complete_job_and_claim_unchanged() {
    let fixture = stalled_run("failed-recovery-claim");
    let before_job = fixture.ordinary.clone();
    let before_claim = fixture
        .db
        .conn
        .query_row(
            "SELECT claim_owner, claim_expires_at_unix_ms, updated_at_unix_ms
             FROM receiver_jobs WHERE job_id = ?1",
            [fixture.job_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .expect("load claim before failed CAS");

    let claimed = fixture
        .db
        .claim_receiver_recovery_run(fixture.job_id, "recovery-owner", 301_399, 331_399)
        .expect("reject early recovery claim");

    assert!(claimed.is_none());
    assert_eq!(
        fixture
            .db
            .receiver_job(fixture.job_id)
            .expect("load job after failed CAS")
            .expect("job after failed CAS"),
        before_job
    );
    let after_claim = fixture
        .db
        .conn
        .query_row(
            "SELECT claim_owner, claim_expires_at_unix_ms, updated_at_unix_ms
             FROM receiver_jobs WHERE job_id = ?1",
            [fixture.job_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .expect("load claim after failed CAS");
    assert_eq!(after_claim, before_claim);
}
