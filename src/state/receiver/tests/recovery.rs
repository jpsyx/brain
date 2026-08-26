#[test]
fn preacceptance_and_delivery_leases_can_be_reclaimed() {
    for state in [
        "claimed",
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
fn reclaiming_delivery_job_preserves_recovery_evidence_and_replaces_lease() {
    for (state, stored_state) in [
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
fn expired_launching_is_an_ambiguity_fence_that_cannot_be_replayed() {
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

    assert!(
        db.claim_next_receiver_run("recovery-worker", 1_100, 1_200)
            .expect("poll ambiguous launch")
            .is_none(),
        "an expired launching row may represent a successful process spawn"
    );
    let preserved = db
        .receiver_job(accepted.job_id())
        .expect("load ambiguous launch")
        .expect("ambiguous launch remains durable");
    assert_eq!(preserved.state(), ReceiverJobState::Launching);
    assert_eq!(preserved.retry_count(), 0);
    assert_eq!(preserved.retry_at_unix_ms(), None);
    assert_eq!(preserved.last_error(), None);
    assert!(
        !db.renew_receiver_claim(accepted.job_id(), "crashed-worker", 1_100, 1_200)
            .expect("stale owner cannot renew")
    );
    assert_eq!(
        db.locked_session_for_instance("crashed-worker", &scope),
        Some(registered.as_str().to_owned())
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
    assert_eq!(registrations, 1);
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
fn expired_observed_lifecycles_remain_unchanged_until_stalled_recovery_exists() {
    for (state, revision, progressing_at_unix_ms) in
        [("accepted", 1_i64, None), ("processing", 2_i64, Some(1_040_i64))]
    {
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
                     claim_expires_at_unix_ms = 1_100,
                     launched_at_unix_ms = 1_020, accepted_at_unix_ms = 1_030,
                     progressing_at_unix_ms = ?2,
                     observation_instance = '11111111-1111-4111-8111-111111111111',
                     observation_session_id = 'native-session', observation_revision = ?3
                 WHERE job_id = ?4",
                rusqlite::params![
                    state,
                    progressing_at_unix_ms,
                    revision,
                    accepted.job_id().to_string(),
                ],
            )
            .expect("seed observed lifecycle");
        let before = db
            .receiver_job(accepted.job_id())
            .expect("load observed job")
            .expect("observed job");

        assert!(
            db.claim_next_receiver_run("fresh-process", 1_100, 1_200)
                .expect("poll expired observed job")
                .is_none(),
            "{state} must not be reclaimed or replayed before stalled recovery exists"
        );
        assert_eq!(
            db.receiver_job(accepted.job_id())
                .expect("reload observed job")
                .expect("observed job remains durable"),
            before,
            "{state} lifecycle evidence changed during restart polling"
        );
    }
}

#[test]
fn expired_launching_blocks_the_fifo_without_consuming_retry_budget() {
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

    assert!(db
        .claim_next_receiver_run("recovery-worker", 1_100, 1_200)
        .expect("poll ambiguous launch")
        .is_none());
    let preserved = db
        .receiver_job(first.job_id())
        .expect("load ambiguous job")
        .expect("ambiguous job remains durable");
    assert_eq!(preserved.state(), ReceiverJobState::Launching);
    assert_eq!(
        preserved.retry_count(),
        MAX_RECEIVER_LAUNCH_ATTEMPTS - 1
    );
    assert_eq!(preserved.last_error(), None);
    assert_eq!(
        db.receiver_job(second.job_id())
            .expect("load blocked FIFO job")
            .expect("second job remains durable")
            .state(),
        ReceiverJobState::Queued
    );
}

#[test]
fn due_delivery_retry_keeps_retrying_state_until_the_new_owner_resumes_delivery() {
    let db = Db::open_in_memory().expect("receiver state");
    let job = receiver_job(None, 100);
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let accepted = db
        .accept_receiver_job(&job, &identity)
        .expect("accept receiver job");
    register_observation_session(&db, accepted.conversation_id(), &job, "instance-a", "session-a");
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
