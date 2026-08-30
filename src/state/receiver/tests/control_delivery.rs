fn sms_identity() -> ReceiverConversationIdentity {
    ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id())
}

fn accept_control(db: &Db, prompt: &str, at: u64) -> ReceiverAcceptance {
    let mut inbound = receiver_job(Some(&format!("control-{at}")), at);
    inbound.prompt = prompt.to_owned();
    db.accept_receiver_job(&inbound, &sms_identity())
        .expect("accept control job")
}

fn response_kinds(db: &Db) -> Vec<(String, String)> {
    let mut statement = db
        .conn
        .prepare(
            "SELECT response_kind, state FROM receiver_deliveries
             ORDER BY created_at_unix_ms, delivery_id",
        )
        .expect("prepare response intent query");
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("query response intents")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("collect response intents")
}

#[test]
fn new_session_commits_its_boundary_and_acknowledgement_intent_atomically() {
    let db = Db::open_in_memory().expect("receiver state");
    let command = accept_control(&db, " /NeW\n", 100);
    let claim = db
        .claim_next_receiver_run("new-owner", 101, 1_101)
        .expect("claim new-session command")
        .expect("new-session claim");

    let records = crate::logging::capture_receiver_lifecycle(|| {
        assert!(
            db.complete_receiver_new_session(command.job_id(), claim.claim().owner(), 102)
                .expect("commit new-session boundary")
        );
    });

    assert_eq!(
        db.receiver_job(command.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::AnswerReady
    );
    assert_eq!(
        response_kinds(&db),
        vec![("control-acknowledgement".to_owned(), "ready".to_owned())]
    );
    assert_receiver_lifecycle_records(
        &records,
        &["receiver lifecycle event=answer-readiness phase=answer-ready cleanup_gated=0"],
    );
}

#[test]
fn restart_commits_one_ack_and_one_notice_for_each_dropped_job_atomically() {
    let db = Db::open_in_memory().expect("receiver state");
    let first = accept_control(&db, "first waiting message", 100);
    let second = accept_control(&db, "second waiting message", 150);
    let restart = accept_control(&db, " /restart ", 200);

    let mut plan = None;
    let records = crate::logging::capture_receiver_lifecycle(|| {
        plan = db.apply_next_receiver_restart(201).expect("commit restart cut");
    });
    let plan = plan.expect("restart plan");

    assert_eq!(plan.dropped.len(), 2);
    for job_id in [first.job_id(), second.job_id(), restart.job_id()] {
        assert_eq!(
            db.receiver_job(job_id).unwrap().unwrap().state(),
            ReceiverJobState::AnswerReady
        );
    }
    let kinds = response_kinds(&db);
    assert_eq!(kinds.len(), 3);
    assert_eq!(
        kinds
            .iter()
            .filter(|(kind, state)| kind == "unavailable-notice" && state == "ready")
            .count(),
        2
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|(kind, state)| kind == "control-acknowledgement" && state == "ready")
            .count(),
        1
    );
    assert_receiver_lifecycle_records(
        &records,
        &["receiver lifecycle event=answer-readiness phase=answer-ready cleanup_gated=0"],
    );
}

#[test]
fn invalid_control_destination_rolls_back_conversation_and_reply_intent() {
    let db = Db::open_in_memory().expect("receiver state");
    let command = accept_control(&db, " /new ", 100);
    let following = accept_control(&db, "after the boundary", 200);
    let claim = db
        .claim_next_receiver_run("new-owner", 201, 1_201)
        .expect("claim new-session command")
        .expect("new-session claim");
    db.conn
        .execute(
            "UPDATE receiver_jobs SET response_sender = '' WHERE job_id = ?1",
            [command.job_id().to_string()],
        )
        .expect("invalidate frozen response destination");

    assert!(
        db.complete_receiver_new_session(command.job_id(), claim.claim().owner(), 202)
            .is_err(),
        "invalid reply intent must fail the whole control transaction"
    );

    let command_job = db.receiver_job(command.job_id()).unwrap().unwrap();
    let following_job = db.receiver_job(following.job_id()).unwrap().unwrap();
    assert_eq!(command_job.state(), ReceiverJobState::Claimed);
    assert_eq!(following_job.conversation_id(), command_job.conversation_id());
    assert!(response_kinds(&db).is_empty());
}
