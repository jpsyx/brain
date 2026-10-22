use super::support::*;
use super::*;

#[test]
fn sms_start_notice_waits_for_frontend_acceptance_and_is_consumed_once() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("state.db");
    let open = || {
        Db::open_path_with_legacy_identity(&path, &receiver_workspace_id().to_string(), "test-user")
            .unwrap()
    };
    let db = open();
    let job = receiver_job(Some("start-notice"), 100);
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let accepted = db.accept_receiver_job(&job, &identity).unwrap();
    assert!(db.pending_receiver_start_notice().unwrap().is_none());
    let claim = db
        .claim_next_receiver_run("worker", 101, 10_000)
        .unwrap()
        .unwrap();
    assert!(
        db.prepare_receiver_job_launch(accepted.job_id(), "worker", 102)
            .unwrap()
    );
    register_observation_session(&db, accepted.conversation_id(), &job, "worker", "session");
    assert!(
        db.commit_receiver_job_launch(
            accepted.job_id(),
            "worker",
            &launch_observation(claim.job().token(), "worker", "session", 103)
        )
        .unwrap()
    );
    assert!(
        db.pending_receiver_start_notice().unwrap().is_none(),
        "spawning is not acceptance"
    );
    assert!(
        db.apply_receiver_observation(
            accepted.job_id(),
            "worker",
            &observation(
                claim.job().token(),
                "worker",
                "session",
                ReceiverNonterminalObservationPhase::Accepted,
                1,
                104
            )
        )
        .unwrap()
    );
    drop(db);
    let db = open();
    let notice = db.pending_receiver_start_notice().unwrap().unwrap();
    assert_eq!(notice.job_id(), accepted.job_id());
    let crate::state::ReceiverDeliveryEnvelope::Sms { value } = notice.envelope() else {
        panic!("expected SMS")
    };
    assert!(
        value.body() == "Started working on your request.",
        "wrong start notice text"
    );
    assert!(db.consume_receiver_start_notice(&notice).unwrap());
    assert!(!db.consume_receiver_start_notice(&notice).unwrap());
    db.accept_receiver_job(&job, &identity).unwrap();
    assert!(
        db.pending_receiver_start_notice().unwrap().is_none(),
        "provider retry must not repeat start notice"
    );
    assert!(
        db.receiver_job(accepted.job_id()).unwrap().unwrap().state() == ReceiverJobState::Accepted,
        "progress delivery changed the agent state"
    );
}

#[test]
fn control_commands_do_not_get_an_agent_start_receipt() {
    let db = Db::open_in_memory().unwrap();
    for prompt in ["/new", "/restart"] {
        let mut job = receiver_job(None, 100);
        job.prompt = prompt.to_owned();
        db.accept_receiver_job(
            &job,
            &ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id()),
        )
        .unwrap();
    }
    let receipts: i64 = db
        .conn
        .query_row(
            "SELECT count(*) FROM meta WHERE key LIKE 'receiver-start:%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(receipts, 0);
}

#[test]
fn email_admission_does_not_create_sms_progress_work() {
    let db = Db::open_in_memory().unwrap();
    let job = receiver_job_for(
        receiver_workspace_id(),
        crate::server::receiver::Channel::Email,
        Some("email"),
        100,
    );
    let identity = ReceiverConversationIdentity::email(
        receiver_workspace_id(),
        receiver_user_id(),
        EmailLineage::verified("email-thread").unwrap(),
    );
    db.accept_receiver_job(&job, &identity).unwrap();
    assert!(db.pending_receiver_start_notice().unwrap().is_none());
}

#[test]
fn receipt_reports_agent_wait_only_for_other_unfinished_jobs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.db");
    let workspace = receiver_workspace_id();
    let db =
        Db::open_path_with_legacy_identity(&path, &workspace.to_string(), "test-user").unwrap();
    let first = receiver_job(None, 100);
    let second = receiver_job(None, 200);
    let identity = ReceiverConversationIdentity::sms(workspace, receiver_user_id());
    db.accept_receiver_job(&first, &identity).unwrap();
    assert!(!Db::receiver_waiting_for_agent_read_only(&path, workspace, first.job_id).unwrap());
    db.accept_receiver_job(&second, &identity).unwrap();
    assert!(Db::receiver_waiting_for_agent_read_only(&path, workspace, second.job_id).unwrap());
    assert!(!Db::receiver_waiting_for_agent_read_only(&path, workspace, first.job_id).unwrap());
}
