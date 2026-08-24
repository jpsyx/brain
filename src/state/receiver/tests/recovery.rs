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
        (ReceiverJobState::Launching, "launching"),
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
    for (expected, next, observed_at) in [
        (ReceiverJobState::Claimed, ReceiverJobState::Launching, 1_010),
        (ReceiverJobState::Launching, ReceiverJobState::Accepted, 1_020),
        (ReceiverJobState::Accepted, ReceiverJobState::Processing, 1_030),
        (
            ReceiverJobState::Processing,
            ReceiverJobState::AnswerReady,
            1_040,
        ),
        (
            ReceiverJobState::AnswerReady,
            ReceiverJobState::Delivering,
            1_050,
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
            1_060,
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
