type RepairedDeliveryRow = (
    String,
    String,
    Option<String>,
    i64,
    Option<i64>,
    Option<String>,
    Option<i64>,
    i64,
    Option<i64>,
    i64,
    i64,
    String,
);

fn seed_later_ready_response(db: &Db, provider_id: &str, created_at_unix_ms: u64) -> ReceiverJobId {
    let accepted = db
        .accept_receiver_job(
            &receiver_job(Some(provider_id), created_at_unix_ms),
            &ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id()),
        )
        .expect("accept later semantic response");
    seed_generic_response(
        db,
        accepted.job_id(),
        "unavailable-notice",
        created_at_unix_ms,
        r#"{"channel":"sms","value":{"sender":"+12125550100","recipient":"+12125550100","body":"later response","long_form_available":false}}"#,
    );
    db.conn
        .execute(
            "UPDATE receiver_jobs SET state = 'answer-ready' WHERE job_id = ?1",
            [accepted.job_id().to_string()],
        )
        .expect("stage later answer-ready response");
    accepted.job_id()
}

#[test]
fn same_version_startup_repairs_every_delivery_identity_time_and_state_shape() {
    let mutations = [
        "delivery_id = 'malformed-delivery-id'",
        "state = 'delivering', attempt_id = 'malformed-attempt-id', claim_owner = 'owner', claim_expires_at_unix_ms = 500",
        "state = 'retrying', attempt_count = -1, retry_at_unix_ms = 100",
        "state = 'retrying', attempt_count = 1, retry_at_unix_ms = -1",
        "state = 'delivering', attempt_id = '10000000-0000-4000-8000-000000000001', claim_owner = 'owner', claim_expires_at_unix_ms = -1",
        "state = 'delivering', attempt_id = '10000000-0000-4000-8000-000000000001', claim_owner = 'owner', claim_expires_at_unix_ms = 500, provider_io_started = 2",
        "state = 'retrying', attempt_count = 1, retry_at_unix_ms = 100, first_attempt_at_unix_ms = -1",
        "created_at_unix_ms = -1",
        "updated_at_unix_ms = -1",
        "state = 'malformed-state'",
    ];
    for (case_index, mutation) in mutations.into_iter().enumerate() {
        let fixture = answer_ready_fixture();
        fixture
            .db
            .conn
            .execute_batch("PRAGMA ignore_check_constraints = ON;")
            .expect("allow corrupt delivery fixture");
        fixture
            .db
            .conn
            .execute_batch(&format!(
                "UPDATE receiver_deliveries SET {mutation} WHERE job_id = '{}';
                 UPDATE receiver_jobs
                 SET state = CASE
                   WHEN (SELECT state FROM receiver_deliveries WHERE job_id = '{}') = 'delivering'
                     THEN 'delivering'
                   WHEN (SELECT state FROM receiver_deliveries WHERE job_id = '{}') = 'retrying'
                     THEN 'retrying'
                   ELSE state END
                 WHERE job_id = '{}';
                 PRAGMA ignore_check_constraints = OFF;",
                fixture.job_id, fixture.job_id, fixture.job_id, fixture.job_id,
            ))
            .expect("stage structurally malformed delivery row");

        super::super::schema::up(&fixture.db.conn, 12)
            .expect("repair structurally malformed delivery row");

        let repaired: RepairedDeliveryRow = fixture
            .db
            .conn
            .query_row(
                "SELECT delivery.delivery_id, delivery.state, delivery.attempt_id,
                        delivery.attempt_count, delivery.retry_at_unix_ms,
                        delivery.claim_owner, delivery.claim_expires_at_unix_ms,
                        delivery.provider_io_started, delivery.first_attempt_at_unix_ms,
                        delivery.created_at_unix_ms, delivery.updated_at_unix_ms, job.state
                 FROM receiver_deliveries AS delivery
                 JOIN receiver_jobs AS job ON job.job_id = delivery.job_id
                 WHERE delivery.job_id = ?1",
                [fixture.job_id.to_string()],
                |row| {
                    Ok((
                        row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?,
                        row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?, row.get(9)?,
                        row.get(10)?, row.get(11)?,
                    ))
                },
            )
            .expect("load repaired delivery row");
        assert!(
            ReceiverDeliveryId::parse(&repaired.0).is_ok(),
            "case {case_index} retained an invalid delivery identity"
        );
        assert!(
            repaired.1 == "failed"
                && repaired.2.is_none()
                && (0..=i64::from(u32::MAX)).contains(&repaired.3)
                && repaired.4.is_none()
                && repaired.5.is_none()
                && repaired.6.is_none()
                && repaired.7 == 0
                && repaired.8.is_none()
                && repaired.9 >= 0
                && repaired.10 >= 0
                && repaired.11 == "failed",
            "case {case_index} retained malformed delivery state"
        );
    }
}

#[test]
fn malformed_oldest_ready_delivery_terminalizes_before_later_fifo_claim() {
    let fixture = answer_ready_fixture();
    let later = seed_later_ready_response(&fixture.db, "later-after-malformed-ready", 200);
    fixture
        .db
        .conn
        .execute(
            "UPDATE receiver_deliveries
             SET delivery_id = 'malformed-delivery-id', created_at_unix_ms = 100
             WHERE job_id = ?1",
            [fixture.job_id.to_string()],
        )
        .expect("stage malformed oldest ready delivery");

    super::super::schema::up(&fixture.db.conn, 12).expect("repair malformed oldest ready row");
    let claim = fixture
        .db
        .claim_next_receiver_delivery("later-owner", 300, 30_300)
        .expect("claim after malformed oldest ready row")
        .expect("later valid response remains claimable");

    assert!(claim.job_id() == later, "malformed oldest row starved later FIFO work");
    assert!(
        fixture
            .db
            .receiver_job(fixture.job_id)
            .expect("load malformed ready job")
            .expect("malformed ready job")
            .state()
            == ReceiverJobState::Failed,
        "malformed oldest ready row was not terminalized"
    );
}

#[test]
fn malformed_due_retry_is_read_only_in_status_then_reconciles_before_later_claim() {
    let temporary = tempfile::tempdir().expect("temporary state directory");
    let path = temporary.path().join("state.db");
    let db = Db::open_path_with_legacy_identity(
        &path,
        &receiver_workspace_id().to_string(),
        receiver_user_id().as_str(),
    )
    .expect("receiver state");
    let first = seed_later_ready_response(&db, "malformed-due-retry", 100);
    let later = seed_later_ready_response(&db, "later-after-malformed-retry", 200);
    db.conn
        .execute_batch("PRAGMA ignore_check_constraints = ON;")
        .expect("allow corrupt retry fixture");
    db.conn
        .execute(
            "UPDATE receiver_deliveries
             SET state = 'retrying', attempt_count = 1,
                 retry_at_unix_ms = 100, first_attempt_at_unix_ms = -1
             WHERE job_id = ?1",
            [first.to_string()],
        )
        .expect("stage malformed due retry delivery");
    db.conn
        .execute(
            "UPDATE receiver_jobs SET state = 'retrying' WHERE job_id = ?1",
            [first.to_string()],
        )
        .expect("stage malformed due retry job");
    db.conn
        .execute_batch("PRAGMA ignore_check_constraints = OFF;")
        .expect("restore delivery constraints");

    let counts = Db::receiver_delivery_counts_read_only(&path).expect("read delivery status");
    assert!(
        counts.retrying() == 1 && counts.answer_ready() == 1,
        "read-only status did not report persisted phases"
    );
    let unchanged: i64 = db
        .conn
        .query_row(
            "SELECT first_attempt_at_unix_ms FROM receiver_deliveries WHERE job_id = ?1",
            [first.to_string()],
            |row| row.get(0),
        )
        .expect("inspect malformed retry after status");
    assert!(unchanged == -1, "read-only status repaired persisted state");

    assert!(
        db.reconcile_expired_receiver_deliveries(300)
            .expect("reconcile malformed due retry")
            == 1,
        "malformed due retry was not terminalized"
    );
    let claim = db
        .claim_next_receiver_delivery("later-owner", 300, 30_300)
        .expect("claim after malformed due retry")
        .expect("later valid response remains claimable");
    assert!(claim.job_id() == later, "malformed due retry starved later FIFO work");
}
