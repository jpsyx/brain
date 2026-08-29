#[test]
fn legacy_pending_unavailable_notice_becomes_one_durable_delivery_intent() {
    let db = Db::open_in_memory().expect("receiver state");
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let inbound = receiver_job(Some("legacy-terminal-notice"), 100);
    let accepted = db
        .accept_receiver_job(&inbound, &identity)
        .expect("accept legacy receiver job");
    db.conn
        .execute(
            "UPDATE receiver_jobs
             SET state = 'failed', pending_unavailable_notice = 1,
                 last_error = 'recovery-attempt-exhausted'
             WHERE job_id = ?1",
            [accepted.job_id().to_string()],
        )
        .expect("seed BR-16 pending notice");

    assert_eq!(
        db.reconcile_expired_receiver_deliveries(200)
            .expect("reconcile legacy notice"),
        1
    );

    let (kind, state, pending): (String, String, bool) = db
        .conn
        .query_row(
            "SELECT delivery.response_kind, delivery.state,
                    job.pending_unavailable_notice
             FROM receiver_deliveries AS delivery
             JOIN receiver_jobs AS job ON job.job_id = delivery.job_id
             WHERE delivery.job_id = ?1",
            [accepted.job_id().to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("load migrated notice intent");
    assert_eq!(kind, "unavailable-notice");
    assert_eq!(state, "ready");
    assert!(!pending);
    assert_eq!(
        db.receiver_job(accepted.job_id())
            .unwrap()
            .unwrap()
            .state(),
        ReceiverJobState::AnswerReady
    );

    assert_eq!(
        db.reconcile_expired_receiver_deliveries(201)
            .expect("repeat reconciliation"),
        0,
        "restart reconciliation must not duplicate the semantic notice"
    );
}

#[test]
fn legacy_pending_notice_storage_failure_preserves_source_for_exact_retry() {
    let db = Db::open_in_memory().expect("receiver state");
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let inbound = receiver_job(Some("legacy-notice-storage-fault"), 100);
    let accepted = db
        .accept_receiver_job(&inbound, &identity)
        .expect("accept legacy receiver job");
    db.conn
        .execute(
            "UPDATE receiver_jobs
             SET state = 'failed', pending_unavailable_notice = 1,
                 last_error = 'recovery-attempt-exhausted'
             WHERE job_id = ?1",
            [accepted.job_id().to_string()],
        )
        .expect("seed BR-16 pending notice");
    db.conn
        .execute_batch(
            "CREATE TRIGGER reject_legacy_notice_insert
             BEFORE INSERT ON receiver_deliveries
             WHEN NEW.response_kind = 'unavailable-notice'
             BEGIN
               SELECT RAISE(ABORT, 'forced unavailable notice insert failure');
             END;",
        )
        .expect("install insert-only fault");

    let error = db
        .reconcile_expired_receiver_deliveries(200)
        .expect_err("storage failure must abort legacy notice migration");

    assert!(
        format!("{error:#}").contains("forced unavailable notice insert failure"),
        "storage error must retain its SQLite cause"
    );
    let source: (String, bool, Option<String>, i64) = db
        .conn
        .query_row(
            "SELECT state, pending_unavailable_notice, last_error,
                    updated_at_unix_ms
             FROM receiver_jobs WHERE job_id = ?1",
            [accepted.job_id().to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("load preserved legacy source");
    assert_eq!(
        source,
        (
            "failed".to_owned(),
            true,
            Some("recovery-attempt-exhausted".to_owned()),
            100,
        )
    );
    let delivery_count: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM receiver_deliveries WHERE job_id = ?1",
            [accepted.job_id().to_string()],
            |row| row.get(0),
        )
        .expect("count rolled-back legacy notices");
    assert_eq!(delivery_count, 0);

    db.conn
        .execute_batch("DROP TRIGGER reject_legacy_notice_insert;")
        .expect("remove insert-only fault");
    assert_eq!(
        db.reconcile_expired_receiver_deliveries(201)
            .expect("retry legacy notice migration"),
        1
    );
    let recovered: (String, bool, i64) = db
        .conn
        .query_row(
            "SELECT job.state, job.pending_unavailable_notice, COUNT(delivery.delivery_id)
             FROM receiver_jobs AS job
             LEFT JOIN receiver_deliveries AS delivery
               ON delivery.job_id = job.job_id
              AND delivery.response_kind = 'unavailable-notice'
             WHERE job.job_id = ?1
             GROUP BY job.job_id",
            [accepted.job_id().to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("load retried legacy notice");
    assert_eq!(recovered, ("answer-ready".to_owned(), false, 1));
}
