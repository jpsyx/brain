#[test]
fn unavailable_notice_handoff_has_one_finite_writer_without_blocking_fifo() {
    let db = Db::open_in_memory().expect("receiver state");
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let terminal_inbound = receiver_job(Some("terminal-notice"), 100);
    let terminal = db
        .accept_receiver_job(&terminal_inbound, &identity)
        .expect("accept terminal receiver job");
    db.conn
        .execute(
            "UPDATE receiver_jobs
             SET state = 'failed', attempt_kind = 'recovery', recovery_count = 1,
                 pending_unavailable_notice = 1,
                 last_error = 'recovery-attempt-exhausted'
             WHERE job_id = ?1",
            [terminal.job_id().to_string()],
        )
        .expect("persist terminal unavailable intent");
    let later = receiver_job(Some("later-work"), 200);
    let later = db
        .accept_receiver_job(&later, &identity)
        .expect("accept later receiver work");
    let terminal = db.receiver_job(terminal.job_id()).unwrap().unwrap();

    let first = db
        .claim_next_receiver_unavailable_notice("first-writer", 91_201, 92_201)
        .expect("claim pending notice")
        .expect("pending notice claim");

    assert_eq!(first.job_id(), terminal.id());
    assert_eq!(first.token(), terminal.token());
    assert_eq!(first.owner(), "first-writer");
    assert_eq!(first.inbound(), terminal.inbound());
    assert!(
        db
            .claim_next_receiver_unavailable_notice("competing-writer", 91_202, 92_202)
            .expect("competing notice claim")
            .is_none()
    );
    assert_eq!(
        db
            .claim_next_receiver_run("ordinary-owner", 91_202, 121_202)
            .expect("ordinary FIFO remains independent")
            .expect("later ordinary work")
            .job()
            .id(),
        later.job_id()
    );

    let retry = db
        .claim_next_receiver_unavailable_notice("retry-writer", 92_201, 93_201)
        .expect("reclaim expired notice")
        .expect("expired notice becomes retryable");
    assert_eq!(retry.job_id(), terminal.id());
    assert!(!db
        .acknowledge_receiver_unavailable_notice(
            retry.job_id(),
            retry.token(),
            "stale-writer",
            92_202,
        )
        .expect("reject stale notice writer"));
    assert!(db
        .acknowledge_receiver_unavailable_notice(
            retry.job_id(),
            retry.token(),
            retry.owner(),
            92_202,
        )
        .expect("acknowledge exact local handoff"));
    assert!(!db
        .receiver_job(terminal.id())
        .unwrap()
        .unwrap()
        .pending_unavailable_notice());
}
