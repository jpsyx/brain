use std::path::Path;

fn seed_delivery(db: &Db, job_id: ReceiverJobId, state: &str) {
    let token: String = db
        .conn
        .query_row(
            "SELECT job_token FROM receiver_jobs WHERE job_id = ?1",
            [job_id.to_string()],
            |row| row.get(0),
        )
        .expect("load token");
    db.conn
        .execute(
            "INSERT INTO receiver_deliveries
               (delivery_id, job_id, job_token, response_kind, envelope_json, state,
                attempt_count, retry_at_unix_ms, created_at_unix_ms, updated_at_unix_ms)
             VALUES (?1, ?2, ?3, 'unavailable-notice', ?4, ?5, 0, ?6, 100, 100)",
            rusqlite::params![
                ReceiverDeliveryId::new().to_string(),
                job_id.to_string(),
                token,
                "private-envelope-canary",
                state,
                (state == "retrying").then_some(200_i64),
            ],
        )
        .expect("seed delivery phase");
}

fn open_file_db(path: &Path) -> Db {
    let workspace_id = receiver_workspace_id().to_string();
    Db::open_path_with_legacy_identity(
        path,
        &workspace_id,
        receiver_user_id().as_str(),
    )
    .expect("file receiver state")
}

#[test]
fn work_summary_reports_agent_recovery_cleanup_and_delivery_from_durable_state() {
    let db = Db::open_in_memory().expect("receiver state");
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let oldest = db
        .accept_receiver_job(&receiver_job(Some("oldest"), 100), &identity)
        .expect("accept oldest");
    let later = db
        .accept_receiver_job(&receiver_job(Some("later"), 200), &identity)
        .expect("accept later");
    db.conn
        .execute(
            "UPDATE receiver_jobs
             SET state = 'processing', recovery_count = 1
             WHERE job_id = ?1",
            [oldest.job_id().to_string()],
        )
        .expect("stage oldest active work");
    seed_delivery(&db, oldest.job_id(), "cleanup-gated");
    seed_delivery(&db, later.job_id(), "retrying");

    let summary = db.receiver_work_summary().expect("work summary");

    assert_eq!(summary.agent_queue_depth(), 2);
    assert_eq!(
        summary.oldest_active_phase(),
        Some(ReceiverWorkPhase::Processing)
    );
    assert_eq!(summary.recovery_attempt(), Some(1));
    assert_eq!(summary.recovery_limit(), MAX_RECEIVER_RECOVERY_ATTEMPTS);
    assert_eq!(summary.cleanup_gated_responses(), 1);
    assert_eq!(summary.delivery_counts().retrying(), 1);
}

#[test]
fn work_summary_does_not_decode_or_expose_content_bearing_columns() {
    let db = Db::open_in_memory().expect("receiver state");
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let accepted = db
        .accept_receiver_job(&receiver_job(Some("private-provider-canary"), 100), &identity)
        .expect("accept private job");
    db.conn
        .execute(
            "UPDATE receiver_jobs
             SET inbound_json = 'not-json-private-prompt-canary',
                 response_sender = 'private-sender-canary',
                 last_error = 'private-error-canary'
             WHERE job_id = ?1",
            [accepted.job_id().to_string()],
        )
        .expect("corrupt private columns");
    seed_delivery(&db, accepted.job_id(), "ready");
    db.conn
        .execute(
            "UPDATE receiver_deliveries
             SET envelope_json = 'not-json-private-answer-canary',
                 provider_reference = 'private-provider-reference-canary'
             WHERE job_id = ?1",
            [accepted.job_id().to_string()],
        )
        .expect("corrupt private delivery columns");

    let summary = db.receiver_work_summary().expect("content-free summary");
    let rendered = format!("{summary:?}");
    let workspace_id = receiver_workspace_id().to_string();

    assert_eq!(summary.agent_queue_depth(), 1);
    for private in [
        "private-prompt-canary",
        "private-sender-canary",
        "private-error-canary",
        "private-answer-canary",
        "private-provider-reference-canary",
        "private-provider-canary",
        workspace_id.as_str(),
    ] {
        assert!(!rendered.contains(private), "summary leaked {private}");
    }
}

#[test]
fn work_summary_rejects_malformed_finite_state_instead_of_hiding_it() {
    let db = Db::open_in_memory().expect("receiver state");
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let accepted = db
        .accept_receiver_job(&receiver_job(Some("malformed"), 100), &identity)
        .expect("accept job");
    db.conn
        .pragma_update(None, "ignore_check_constraints", true)
        .expect("allow corruption fixture");
    db.conn
        .execute(
            "UPDATE receiver_jobs SET state = 'mystery' WHERE job_id = ?1",
            [accepted.job_id().to_string()],
        )
        .expect("stage malformed state");

    let error = db
        .receiver_work_summary()
        .expect_err("malformed state must be unavailable");

    assert!(error.to_string().contains("receiver job state"), "{error:#}");
}

#[test]
fn read_only_work_summary_distinguishes_missing_state_without_creating_it() {
    let temporary = tempfile::tempdir().expect("state directory");
    let missing = temporary.path().join("missing.db");

    let summary = Db::receiver_work_summary_read_only(&missing, receiver_workspace_id())
        .expect("missing read-only state");

    assert_eq!(summary, None);
    assert!(!missing.exists(), "read-only summary created state");
}

#[test]
fn read_only_work_summary_preserves_database_bytes_and_returns_available_state() {
    let temporary = tempfile::tempdir().expect("state directory");
    let path = temporary.path().join("state.db");
    let db = open_file_db(&path);
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    db.accept_receiver_job(&receiver_job(Some("read-only"), 100), &identity)
        .expect("accept job");
    drop(db);
    let before = std::fs::read(&path).expect("snapshot database");

    let summary = Db::receiver_work_summary_read_only(&path, receiver_workspace_id())
        .expect("read-only summary")
        .expect("available summary");

    assert_eq!(summary.agent_queue_depth(), 1);
    assert_eq!(std::fs::read(&path).expect("database after read"), before);
}
