#[test]
fn ordinary_claim_establishes_a_launch_lease_without_consuming_recovery() {
    let db = Db::open_in_memory().expect("receiver state");
    let inbound = receiver_job(Some("recovery-state-claim"), 100);
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let accepted = db
        .accept_receiver_job(&inbound, &identity)
        .expect("accept receiver job");

    let run = db
        .claim_next_receiver_run("worker-a", 1_000, 1_100)
        .expect("claim ordinary run")
        .expect("ordinary run");
    let job = run.job();

    assert_eq!(job.attempt_kind(), ReceiverAttemptKind::Ordinary);
    assert_eq!(job.launch_expires_at_unix_ms(), Some(121_000));
    assert_eq!(job.acceptance_expires_at_unix_ms(), None);
    assert_eq!(job.progress_expires_at_unix_ms(), None);
    assert_eq!(job.recovery_expires_at_unix_ms(), None);
    assert_eq!(job.absolute_work_expires_at_unix_ms(), None);
    assert_eq!(job.latest_progress_at_unix_ms(), None);
    assert_eq!(job.recovery_count(), 0);
    assert!(!job.pending_unavailable_notice());
    assert_eq!(job.id(), accepted.job_id());
}

#[test]
fn exact_lifecycle_commits_establish_bounded_deadlines_from_authorization_time() {
    let db = Db::open_in_memory().expect("receiver state");
    let inbound = receiver_job(Some("recovery-state-lifecycle"), 100);
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let accepted = db
        .accept_receiver_job(&inbound, &identity)
        .expect("accept receiver job");
    db.claim_next_receiver_run("instance-a", 1_000, 10_000)
        .expect("claim ordinary run")
        .expect("ordinary run");
    assert!(
        db.prepare_receiver_job_launch(accepted.job_id(), "instance-a", 1_010)
            .expect("prepare launch")
    );
    register_observation_session(
        &db,
        accepted.conversation_id(),
        &inbound,
        "instance-a",
        "session-a",
    );
    let token = db
        .receiver_job(accepted.job_id())
        .expect("load receiver job")
        .expect("receiver job")
        .token();
    let launch = ReceiverLaunchObservation {
        token,
        instance: "instance-a".to_owned(),
        session_id: "session-a".to_owned(),
        observed_at_unix_ms: 50_000,
        authorized_at_unix_ms: 1_020,
    };
    assert!(
        db.commit_receiver_job_launch(accepted.job_id(), "instance-a", &launch)
            .expect("commit launch")
    );
    assert_eq!(
        db.receiver_job(accepted.job_id())
            .expect("load launched job")
            .expect("launched job")
            .acceptance_expires_at_unix_ms(),
        Some(91_020),
        "future producer evidence must not extend the acceptance lease"
    );

    let accepted_observation = ReceiverObservation {
        token,
        instance: "instance-a".to_owned(),
        session_id: "session-a".to_owned(),
        phase: ReceiverNonterminalObservationPhase::Accepted,
        revision: 1,
        observed_at_unix_ms: 60_000,
        authorized_at_unix_ms: 1_030,
    };
    assert!(
        db.apply_receiver_observation(
            accepted.job_id(),
            "instance-a",
            &accepted_observation,
        )
        .expect("commit accepted observation")
    );
    let accepted_job = db
        .receiver_job(accepted.job_id())
        .expect("load accepted job")
        .expect("accepted job");
    assert_eq!(accepted_job.accepted_at_unix_ms(), Some(60_000));
    assert_eq!(accepted_job.attempt_accepted_at_unix_ms(), Some(60_000));
    assert_eq!(accepted_job.latest_progress_at_unix_ms(), None);
    assert_eq!(accepted_job.progress_expires_at_unix_ms(), Some(301_030));
    assert_eq!(
        accepted_job.absolute_work_expires_at_unix_ms(),
        Some(1_801_030)
    );

    let progress_observation = ReceiverObservation {
        phase: ReceiverNonterminalObservationPhase::Progressing,
        revision: 2,
        observed_at_unix_ms: 70_000,
        authorized_at_unix_ms: 1_040,
        ..accepted_observation
    };
    assert!(
        db.apply_receiver_observation(
            accepted.job_id(),
            "instance-a",
            &progress_observation,
        )
        .expect("commit progress observation")
    );
    let processing = db
        .receiver_job(accepted.job_id())
        .expect("load processing job")
        .expect("processing job");
    assert_eq!(processing.progressing_at_unix_ms(), Some(70_000));
    assert_eq!(processing.attempt_progressing_at_unix_ms(), Some(70_000));
    assert_eq!(processing.latest_progress_at_unix_ms(), Some(70_000));
    assert_eq!(processing.progress_expires_at_unix_ms(), Some(301_040));
    assert_eq!(
        processing.absolute_work_expires_at_unix_ms(),
        Some(1_801_030),
        "progress must not renew the immutable absolute limit"
    );
}

#[test]
fn recovery_attempt_snapshot_preserves_lifetime_identity_and_uses_a_fresh_cursor() {
    let db = Db::open_in_memory().expect("receiver state");
    let inbound = receiver_job(Some("recovery-state-attempt"), 100);
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let accepted = db
        .accept_receiver_job(&inbound, &identity)
        .expect("accept receiver job");
    let original = db
        .receiver_job(accepted.job_id())
        .expect("load original job")
        .expect("original job");
    db.conn
        .execute(
            "UPDATE receiver_jobs
             SET state = 'retrying', retry_count = ?1, retry_at_unix_ms = 2_000,
                 retry_from_state = 'accepted', accepted_at_unix_ms = 1_000,
                 progressing_at_unix_ms = 1_100, attempt_accepted_at_unix_ms = NULL,
                 attempt_progressing_at_unix_ms = NULL, latest_progress_at_unix_ms = NULL,
                 observation_instance = NULL, observation_session_id = NULL,
                 observation_revision = 0, progress_expires_at_unix_ms = 2_100,
                 recovery_expires_at_unix_ms = 2_500,
                 absolute_work_expires_at_unix_ms = 3_000,
                 recovery_count = ?2, attempt_kind = 'recovery',
                 pending_unavailable_notice = 1
             WHERE job_id = ?3",
            rusqlite::params![
                i64::from(MAX_RECEIVER_LAUNCH_ATTEMPTS),
                i64::from(MAX_RECEIVER_RECOVERY_ATTEMPTS),
                accepted.job_id().to_string(),
            ],
        )
        .expect("seed recovery attempt");

    let recovery = db
        .receiver_job(accepted.job_id())
        .expect("load recovery attempt")
        .expect("recovery attempt");
    assert_eq!(recovery.id(), original.id());
    assert_eq!(recovery.token(), original.token());
    assert_eq!(recovery.conversation_id(), original.conversation_id());
    assert_eq!(recovery.inbound(), original.inbound());
    assert_eq!(recovery.accepted_at_unix_ms(), Some(1_000));
    assert_eq!(recovery.progressing_at_unix_ms(), Some(1_100));
    assert_eq!(recovery.attempt_accepted_at_unix_ms(), None);
    assert_eq!(recovery.attempt_progressing_at_unix_ms(), None);
    assert_eq!(recovery.observation_revision(), 0);
    assert_eq!(
        recovery
            .observation_cursor()
            .expect("fresh recovery observation cursor"),
        crate::agent::AgentObservationCursor::launched(),
    );
    assert_eq!(recovery.attempt_kind(), ReceiverAttemptKind::Recovery);
    assert_eq!(recovery.recovery_count(), MAX_RECEIVER_RECOVERY_ATTEMPTS);
    assert!(recovery.pending_unavailable_notice());
    assert_eq!(
        recovery.recovery_snapshot(2_000),
        ReceiverRecoverySnapshot {
            state: ReceiverJobState::Retrying,
            attempt_kind: ReceiverAttemptKind::Recovery,
            launch_attempt_count: MAX_RECEIVER_LAUNCH_ATTEMPTS,
            recovery_count: MAX_RECEIVER_RECOVERY_ATTEMPTS,
            now_unix_ms: 2_000,
            launch_expires_at_unix_ms: None,
            acceptance_expires_at_unix_ms: None,
            progress_expires_at_unix_ms: Some(2_100),
            recovery_expires_at_unix_ms: Some(2_500),
            absolute_work_expires_at_unix_ms: Some(3_000),
        }
    );
}
