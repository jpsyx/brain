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

fn acknowledge_stalled_cleanup(fixture: &StalledRunFixture, now_unix_ms: u64) {
    assert!(
        fixture
            .db
            .acknowledge_receiver_recovery_cleanup(
                fixture.job_id,
                fixture.ordinary.token(),
                "ordinary-instance",
                "native-session",
                now_unix_ms,
            )
            .expect("acknowledge stalled-run cleanup")
    );
}

#[test]
fn ordinary_launch_preparation_rejects_a_claimed_recovery_attempt() {
    let fixture = stalled_run("ordinary-prepare-rejects-recovery");
    fixture
        .db
        .reconcile_next_receiver_job(301_400)
        .expect("persist due recovery")
        .expect("recovery effect");
    acknowledge_stalled_cleanup(&fixture, 301_401);
    fixture
        .db
        .claim_receiver_recovery_run(fixture.job_id, "recovery-owner", 301_401, 331_401)
        .expect("claim recovery")
        .expect("recovery claim");

    assert!(
        !fixture
            .db
            .prepare_receiver_job_launch(fixture.job_id, "recovery-owner", 301_402)
            .expect("ordinary preparation rejects recovery")
    );
    assert_eq!(
        fixture
            .db
            .receiver_job(fixture.job_id)
            .expect("load recovery after rejected ordinary preparation")
            .expect("recovery after rejected ordinary preparation")
            .state(),
        ReceiverJobState::Claimed
    );
    assert!(
        fixture
            .db
            .prepare_receiver_recovery_job_launch(
                fixture.job_id,
                "recovery-owner",
                301_403,
            )
            .expect("recovery preparation accepts recovery")
    );
    let scope = crate::agent::SessionScope::new(
        crate::agent::AgentKind::Codex,
        fixture.inbound.workspace_id,
        fixture.inbound.actor.clone(),
    );
    let session = crate::agent::AgentSession::new("native-session").expect("native session");
    fixture
        .db
        .claim_receiver_session(
            fixture.ordinary.conversation_id(),
            &session,
            "recovery-instance",
            43,
            &scope,
        )
        .expect("claim recovery native session")
        .expect("recovery registration");
    assert!(
        !fixture
            .db
            .commit_receiver_job_launch(
                fixture.job_id,
                "recovery-owner",
                &launch_observation(
                    fixture.ordinary.token(),
                    "recovery-instance",
                    "native-session",
                    301_404,
                ),
            )
            .expect("ordinary launch commit rejects recovery")
    );
    assert_eq!(
        fixture
            .db
            .receiver_job(fixture.job_id)
            .expect("load recovery after rejected ordinary commit")
            .expect("recovery after rejected ordinary commit")
            .state(),
        ReceiverJobState::Launching
    );
    assert!(
        fixture
            .db
            .commit_receiver_recovery_job_launch(
                fixture.job_id,
                "recovery-owner",
                &launch_observation(
                    fixture.ordinary.token(),
                    "recovery-instance",
                    "native-session",
                    301_405,
                ),
            )
            .expect("recovery launch commit accepts recovery")
    );
}

#[test]
fn recovery_claim_preserves_identity_resets_the_cursor_and_consumes_only_recovery_budget() {
    let fixture = stalled_run("durable-recovery-claim");
    let effect = fixture
        .db
        .reconcile_next_receiver_job(301_400)
        .expect("reconcile stalled work")
        .expect("persist due recovery");
    assert_eq!(
        effect.action(),
        ReceiverReconciliationAction::ScheduleRecovery
    );
    acknowledge_stalled_cleanup(&fixture, 301_401);

    let claimed = fixture
        .db
        .claim_receiver_recovery_run(fixture.job_id, "recovery-owner", 301_401, 331_401)
        .expect("claim recovery run")
        .expect("recovery run");
    let recovery = claimed.job();

    assert_eq!(claimed.claim().owner(), "recovery-owner");
    assert_eq!(claimed.claim().expires_at_unix_ms(), 331_401);
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
    assert_eq!(recovery.launch_expires_at_unix_ms(), Some(421_401));
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

#[test]
fn older_ordinary_claim_blocks_recovery_before_and_after_its_lease_expires() {
    let db = Db::open_in_memory().expect("receiver state");
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let competing_inbound = receiver_job(Some("competing-live-claim"), 50);
    let competing = db
        .accept_receiver_job(&competing_inbound, &identity)
        .expect("accept competing job");
    db.claim_next_receiver_run("competing-owner", 1_000, 2_000)
        .expect("claim competing job")
        .expect("competing claim");
    assert_eq!(
        db.record_receiver_launch_retry(
            competing.job_id(),
            "competing-owner",
            1_100,
            301_000,
            ReceiverLaunchFailure::Planning,
        )
        .expect("schedule competing retry"),
        Some(ReceiverLaunchRetryOutcome::Scheduled)
    );

    let target_inbound = receiver_job(Some("recovery-target"), 100);
    let target = db
        .accept_receiver_job(&target_inbound, &identity)
        .expect("accept recovery target");
    db.claim_next_receiver_run("ordinary-owner", 1_200, 2_000)
        .expect("claim recovery target")
        .expect("target claim");
    assert!(
        db.prepare_receiver_job_launch(target.job_id(), "ordinary-owner", 1_250)
            .expect("prepare target launch")
    );
    register_observation_session(
        &db,
        target.conversation_id(),
        &target_inbound,
        "ordinary-instance",
        "native-session",
    );
    let target_token = db
        .receiver_job(target.job_id())
        .expect("load target")
        .expect("target job")
        .token();
    assert!(
        db.commit_receiver_job_launch(
            target.job_id(),
            "ordinary-owner",
            &launch_observation(
                target_token,
                "ordinary-instance",
                "native-session",
                1_300,
            ),
        )
        .expect("commit target launch")
    );
    assert!(
        db.apply_receiver_observation(
            target.job_id(),
            "ordinary-owner",
            &observation(
                target_token,
                "ordinary-instance",
                "native-session",
                ReceiverNonterminalObservationPhase::Accepted,
                1,
                1_350,
            ),
        )
        .expect("commit target acceptance")
    );
    assert!(
        db.apply_receiver_observation(
            target.job_id(),
            "ordinary-owner",
            &observation(
                target_token,
                "ordinary-instance",
                "native-session",
                ReceiverNonterminalObservationPhase::Progressing,
                2,
                1_400,
            ),
        )
        .expect("commit target progress")
    );
    let competing_claim = db
        .claim_next_receiver_run("competing-owner", 301_399, 331_400)
        .expect("claim due competing retry")
        .expect("competing retry claim");
    assert_eq!(competing_claim.job().id(), competing.job_id());
    assert_eq!(
        db.reconcile_next_receiver_job(301_400)
            .expect("persist recovery behind competing live claim")
            .expect("recovery effect")
            .action(),
        ReceiverReconciliationAction::ScheduleRecovery
    );
    assert!(
        db.acknowledge_receiver_recovery_cleanup(
            target.job_id(),
            target_token,
            "ordinary-instance",
            "native-session",
            301_401,
        )
        .expect("acknowledge target cleanup")
    );

    let target_before = db
        .receiver_job(target.job_id())
        .expect("load target before rejected recovery")
        .expect("target before rejected recovery");
    let competing_before = db
        .receiver_job(competing.job_id())
        .expect("load competitor before rejected recovery")
        .expect("competitor before rejected recovery");
    let claims_before: Vec<(String, Option<String>, Option<i64>, i64)> = db
        .conn
        .prepare(
            "SELECT job_id, claim_owner, claim_expires_at_unix_ms, updated_at_unix_ms
             FROM receiver_jobs WHERE workspace_id = ?1 ORDER BY job_id",
        )
        .expect("prepare claim snapshot")
        .query_map([receiver_workspace_id().to_string()], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .expect("read claims before rejected recovery")
        .collect::<rusqlite::Result<_>>()
        .expect("collect claims before rejected recovery");

    assert!(
        db.claim_receiver_recovery_run(
            target.job_id(),
            "recovery-owner",
            301_400,
            331_400,
        )
        .expect("reject recovery behind live competing claim")
        .is_none(),
        "a workspace may have only one live receiver claim"
    );
    assert_eq!(
        db.receiver_job(target.job_id())
            .expect("reload target after rejected recovery")
            .expect("target after rejected recovery"),
        target_before
    );
    assert_eq!(
        db.receiver_job(competing.job_id())
            .expect("reload competitor after rejected recovery")
            .expect("competitor after rejected recovery"),
        competing_before
    );
    let claims_after: Vec<(String, Option<String>, Option<i64>, i64)> = db
        .conn
        .prepare(
            "SELECT job_id, claim_owner, claim_expires_at_unix_ms, updated_at_unix_ms
             FROM receiver_jobs WHERE workspace_id = ?1 ORDER BY job_id",
        )
        .expect("prepare unchanged claim snapshot")
        .query_map([receiver_workspace_id().to_string()], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .expect("read claims after rejected recovery")
        .collect::<rusqlite::Result<_>>()
        .expect("collect claims after rejected recovery");
    assert_eq!(claims_after, claims_before);

    assert!(
        db.claim_receiver_recovery_run(
            target.job_id(),
            "recovery-owner",
            331_400,
            361_400,
        )
        .expect("keep recovery behind older due ordinary work")
        .is_none(),
        "the older ordinary retry must retain FIFO priority after lease expiry"
    );
}
