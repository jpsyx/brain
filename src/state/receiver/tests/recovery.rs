#[test]
fn every_expired_nonterminal_lease_can_be_reclaimed() {
    for state in [
        "claimed",
        "launching",
        "accepted",
        "processing",
        "answer-ready",
        "delivering",
        "retrying",
    ] {
        let db = Db::open_in_memory().expect("receiver state");
        let job = receiver_job(None, 100);
        let identity =
            ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
        let accepted = db
            .accept_receiver_job(&job, &identity)
            .expect("accept receiver job");
        db.conn
            .execute(
                "UPDATE receiver_jobs
                 SET state = ?1, claim_owner = 'crashed-worker',
                     claim_expires_at_unix_ms = 1_100
                 WHERE job_id = ?2",
                rusqlite::params![state, accepted.job_id().to_string()],
            )
            .expect("seed expired leased state");

        let reclaimed = db
            .claim_next_receiver_job("recovery-worker", 1_100, 1_200)
            .expect("reclaim expired lease")
            .unwrap_or_else(|| panic!("{state} must be reclaimable"));
        assert_eq!(reclaimed.job_id(), accepted.job_id());
    }
}

#[test]
fn reclaiming_progressed_job_preserves_recovery_evidence_and_replaces_lease() {
    for (state, stored_state) in [
        (ReceiverJobState::Accepted, "accepted"),
        (ReceiverJobState::Processing, "processing"),
        (ReceiverJobState::AnswerReady, "answer-ready"),
        (ReceiverJobState::Delivering, "delivering"),
    ] {
        let db = Db::open_in_memory().expect("receiver state");
        let job = receiver_job(None, 100);
        let identity =
            ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
        let accepted = db
            .accept_receiver_job(&job, &identity)
            .expect("accept receiver job");
        db.conn
            .execute(
                "UPDATE receiver_jobs
                 SET state = ?1, retry_count = 2, retry_at_unix_ms = 2_000,
                     last_error = 'previous-attempt', claim_owner = 'crashed-worker',
                     claim_expires_at_unix_ms = 1_100
                 WHERE job_id = ?2",
                rusqlite::params![stored_state, accepted.job_id().to_string()],
            )
            .expect("seed progressed leased state");

        let reclaimed = db
            .claim_next_receiver_job("recovery-worker", 1_100, 1_200)
            .expect("reclaim expired lease")
            .unwrap_or_else(|| panic!("{stored_state} must be reclaimable"));

        assert_eq!(reclaimed.job_id(), accepted.job_id());
        assert_eq!(reclaimed.owner(), "recovery-worker");
        assert_eq!(reclaimed.expires_at_unix_ms(), 1_200);
        let persisted = db
            .receiver_job(accepted.job_id())
            .expect("load reclaimed job")
            .expect("reclaimed job");
        assert_eq!(persisted.state(), state);
        assert_eq!(persisted.retry_count(), 2);
        assert_eq!(persisted.retry_at_unix_ms(), Some(2_000));
        assert_eq!(persisted.last_error(), Some("previous-attempt"));
        let (owner, expiry) = db
            .conn
            .query_row(
                "SELECT claim_owner, claim_expires_at_unix_ms
                 FROM receiver_jobs WHERE job_id = ?1",
                [accepted.job_id().to_string()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .expect("load replacement lease");
        assert_eq!(owner, "recovery-worker");
        assert_eq!(expiry, 1_200);
    }
}

#[test]
fn expired_launching_is_atomically_recovered_as_a_due_spawn_retry() {
    use crate::agent::{AgentKind, AgentSession, SessionScope};

    let db = Db::open_in_memory().expect("receiver state");
    let job = receiver_job(None, 100);
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let accepted = db
        .accept_receiver_job(&job, &identity)
        .expect("accept receiver job");
    db.claim_next_receiver_run("crashed-worker", 1_000, 1_100)
        .expect("claim launch")
        .expect("launch claim");
    assert!(
        db.prepare_receiver_job_launch(accepted.job_id(), "crashed-worker", 1_010)
            .expect("prepare launch")
    );
    let scope = SessionScope::new(AgentKind::Claude, receiver_workspace_id(), job.actor);
    let registered = AgentSession::new("pending-crashed-session").expect("registered session");
    db.register_receiver_session(
        accepted.conversation_id(),
        &registered,
        "crashed-worker",
        42,
        &scope,
    )
    .expect("register crashed launch");

    let recovered = db
        .claim_next_receiver_run("recovery-worker", 1_100, 1_200)
        .expect("recover expired launch")
        .expect("launch retry is immediately due");

    assert_eq!(recovered.claim().owner(), "recovery-worker");
    assert_eq!(recovered.job().state(), ReceiverJobState::Retrying);
    assert_eq!(recovered.job().retry_count(), 1);
    assert_eq!(recovered.job().retry_at_unix_ms(), Some(1_100));
    assert_eq!(
        recovered.job().retry_from_state(),
        Some(ReceiverJobState::Launching)
    );
    assert_eq!(recovered.job().last_error(), Some("launch-spawn"));
    assert!(
        !db.renew_receiver_claim(accepted.job_id(), "crashed-worker", 1_100, 1_200)
            .expect("stale owner cannot renew")
    );
    assert!(
        db.locked_session_for_instance("crashed-worker", &scope)
            .is_none()
    );
    let registrations: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM receiver_session_registrations
             WHERE workspace_id = ?1 AND brain_instance_id = 'crashed-worker'",
            [receiver_workspace_id().to_string()],
            |row| row.get(0),
        )
        .expect("count stale registrations");
    assert_eq!(registrations, 0);
}

#[test]
fn expired_launched_lease_remains_exactly_unchanged_and_blocks_replay() {
    use crate::agent::{AgentKind, AgentSession, SessionScope};

    let db = Db::open_in_memory().expect("receiver state");
    let job = receiver_job(None, 100);
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let accepted = db
        .accept_receiver_job(&job, &identity)
        .expect("accept receiver job");
    db.claim_next_receiver_run("crashed-worker", 1_000, 1_100)
        .expect("claim launch")
        .expect("launch claim");
    assert!(
        db.prepare_receiver_job_launch(accepted.job_id(), "crashed-worker", 1_010)
            .expect("prepare launch")
    );
    let scope = SessionScope::new(AgentKind::Claude, receiver_workspace_id(), job.actor);
    let registered = AgentSession::new("pending-crashed-session").expect("registered session");
    db.register_receiver_session(
        accepted.conversation_id(),
        &registered,
        "crashed-worker",
        42,
        &scope,
    )
    .expect("register launched session");
    let token = db
        .receiver_job(accepted.job_id())
        .expect("load launching job")
        .expect("launching job")
        .token();
    assert!(
        db.commit_receiver_job_launch(
            accepted.job_id(),
            "crashed-worker",
            &launch_observation(token, "crashed-worker", registered.as_str(), 1_020),
        )
        .expect("commit launched boundary")
    );
    let durable_before = db
        .receiver_job(accepted.job_id())
        .expect("load launched job")
        .expect("launched job");
    let correlation_before = db.locked_session_for_instance("crashed-worker", &scope);

    assert!(
        db.claim_next_receiver_run("recovery-worker", 1_100, 1_200)
            .expect("poll expired launched job")
            .is_none(),
        "an expired post-spawn lease must not become launchable"
    );
    assert_eq!(
        db.receiver_job(accepted.job_id()).unwrap().unwrap(),
        durable_before,
        "lease expiry must preserve the complete durable launched row"
    );
    assert_eq!(
        db.locked_session_for_instance("crashed-worker", &scope),
        correlation_before,
        "lease expiry must preserve exact durable session correlation"
    );
}

#[test]
fn exhausted_expired_launch_fails_atomically_then_allows_the_next_fifo_job() {
    let db = Db::open_in_memory().expect("receiver state");
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let first = db
        .accept_receiver_job(&receiver_job(Some("first"), 100), &identity)
        .expect("accept first job");
    let second = db
        .accept_receiver_job(&receiver_job(Some("second"), 200), &identity)
        .expect("accept second job");
    db.conn
        .execute(
            "UPDATE receiver_jobs
             SET state = 'launching', retry_count = ?1,
                 claim_owner = 'crashed-worker', claim_expires_at_unix_ms = 1_100
             WHERE job_id = ?2",
            rusqlite::params![
                i64::from(MAX_RECEIVER_LAUNCH_ATTEMPTS - 1),
                first.job_id().to_string(),
            ],
        )
        .expect("seed exhausted expired launch");

    assert!(
        db.claim_next_receiver_run("recovery-worker", 1_100, 1_200)
            .expect("exhaust expired launch")
            .is_none(),
        "the exhausting transaction must not overtake the oldest job"
    );
    let exhausted = db
        .receiver_job(first.job_id())
        .expect("load exhausted job")
        .expect("exhausted job remains durable");
    assert_eq!(exhausted.state(), ReceiverJobState::Failed);
    assert_eq!(exhausted.retry_count(), MAX_RECEIVER_LAUNCH_ATTEMPTS);
    assert_eq!(exhausted.last_error(), Some("launch-spawn"));

    let next = db
        .claim_next_receiver_run("recovery-worker", 1_100, 1_200)
        .expect("claim next FIFO job")
        .expect("next job is ready on a later poll");
    assert_eq!(next.job().id(), second.job_id());
}

#[test]
fn due_delivery_retry_keeps_retrying_state_until_the_new_owner_resumes_delivery() {
    let db = Db::open_in_memory().expect("receiver state");
    let job = receiver_job(None, 100);
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let accepted = db
        .accept_receiver_job(&job, &identity)
        .expect("accept receiver job");
    db.claim_next_receiver_job("worker-a", 1_000, 1_500)
        .expect("claim job")
        .expect("claim available");
    assert!(db
        .transition_receiver_job(
            accepted.job_id(), "worker-a", ReceiverJobState::Claimed,
            ReceiverJobState::Launching, 1_010,
        )
        .expect("prepare delivery launch"));
    let token = db
        .receiver_job(accepted.job_id())
        .expect("load launch")
        .expect("job")
        .token();
    assert!(db
        .commit_receiver_job_launch(accepted.job_id(), "worker-a", &launch_observation(token, "instance-a", "session-a", 1_020))
        .expect("commit delivery launch"));
    assert!(db
        .apply_receiver_observation(
            accepted.job_id(),
            "worker-a",
            &observation(
                token,
                "instance-a",
                "session-a",
                ReceiverNonterminalObservationPhase::Accepted,
                1,
                1_030,
            ),
        )
        .expect("record delivery acceptance"));
    assert!(db
        .apply_receiver_observation(
            accepted.job_id(),
            "worker-a",
            &observation(
                token,
                "instance-a",
                "session-a",
                ReceiverNonterminalObservationPhase::Progressing,
                2,
                1_040,
            ),
        )
        .expect("record delivery progress"));
    for (expected, next, observed_at) in [
        (
            ReceiverJobState::Processing,
            ReceiverJobState::AnswerReady,
            1_050,
        ),
        (
            ReceiverJobState::AnswerReady,
            ReceiverJobState::Delivering,
            1_060,
        ),
    ] {
        assert!(
            db.transition_receiver_job(
                accepted.job_id(),
                "worker-a",
                expected,
                next,
                observed_at,
            )
            .expect("advance delivery job")
        );
    }
    assert!(
        db.record_receiver_retry(
            accepted.job_id(),
            "worker-a",
            ReceiverJobState::Delivering,
            1_070,
            2_000,
            "delivery-unavailable",
        )
        .expect("record delivery retry")
    );

    let reclaimed = db
        .claim_next_receiver_job("worker-b", 2_000, 2_100)
        .expect("claim due delivery retry")
        .expect("retry is due");
    assert_eq!(reclaimed.job_id(), accepted.job_id());
    let retrying = db
        .receiver_job(accepted.job_id())
        .expect("load claimed retry")
        .expect("retry remains durable");
    assert_eq!(retrying.state(), ReceiverJobState::Retrying);
    assert_eq!(retrying.retry_at_unix_ms(), Some(2_000));
    assert!(
        db.transition_receiver_job(
            accepted.job_id(),
            "worker-b",
            ReceiverJobState::Retrying,
            ReceiverJobState::Delivering,
            2_050,
        )
        .expect("resume delivery as live owner")
    );
    let resumed = db
        .receiver_job(accepted.job_id())
        .expect("load resumed delivery")
        .expect("delivery remains durable");
    assert_eq!(resumed.retry_at_unix_ms(), None);
}

#[test]
fn terminal_jobs_are_never_reclaimed_after_lease_expiry() {
    for state in ["failed", "done"] {
        let db = Db::open_in_memory().expect("receiver state");
        let job = receiver_job(None, 100);
        let identity =
            ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
        let accepted = db
            .accept_receiver_job(&job, &identity)
            .expect("accept receiver job");
        db.conn
            .execute(
                "UPDATE receiver_jobs
                 SET state = ?1, claim_owner = 'old-worker',
                     claim_expires_at_unix_ms = 1_100
                 WHERE job_id = ?2",
                rusqlite::params![state, accepted.job_id().to_string()],
            )
            .expect("seed terminal state");

        assert!(
            db.claim_next_receiver_job("recovery-worker", 1_100, 1_200)
                .expect("poll terminal job")
                .is_none(),
            "{state} must remain terminal"
        );
    }
}

#[test]
fn retry_counter_refuses_to_increment_beyond_u32() {
    let db = Db::open_in_memory().expect("receiver state");
    let job = receiver_job(None, 100);
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let accepted = db
        .accept_receiver_job(&job, &identity)
        .expect("accept receiver job");
    db.claim_next_receiver_job("worker-a", 1_000, 1_100)
        .expect("claim job")
        .expect("claim available");
    db.conn
        .execute(
            "UPDATE receiver_jobs SET retry_count = ?1 WHERE job_id = ?2",
            rusqlite::params![i64::from(u32::MAX), accepted.job_id().to_string()],
        )
        .expect("seed maximum retry count");

    assert!(
        db.record_receiver_retry(
            accepted.job_id(),
            "worker-a",
            ReceiverJobState::Claimed,
            1_050,
            2_000,
            "launch-unavailable",
        )
        .is_err()
    );
    assert_eq!(
        db.receiver_job(accepted.job_id())
            .expect("load receiver job")
            .expect("receiver job")
            .retry_count(),
        u32::MAX
    );
}
