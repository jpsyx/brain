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
