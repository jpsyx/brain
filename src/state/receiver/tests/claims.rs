#[test]
fn expired_receiver_claim_is_reassigned_without_deleting_the_job() {
    let db = Db::open_in_memory().expect("receiver state");
    let job = receiver_job(None, 100);
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let accepted = db
        .accept_receiver_job(&job, &identity)
        .expect("accept receiver job");

    let first = db
        .claim_next_receiver_job("worker-a", 1_000, 1_100)
        .expect("claim oldest job")
        .expect("claim available");
    assert_eq!(first.job_id(), accepted.job_id());
    assert_eq!(first.owner(), "worker-a");
    assert_eq!(first.expires_at_unix_ms(), 1_100);
    assert!(
        db.claim_next_receiver_job("worker-b", 1_099, 1_200)
            .expect("poll before expiry")
            .is_none()
    );

    let reassigned = db
        .claim_next_receiver_job("worker-b", 1_100, 1_200)
        .expect("reclaim expired job")
        .expect("expired claim is available");
    assert_eq!(reassigned.job_id(), accepted.job_id());
    assert_eq!(reassigned.owner(), "worker-b");
    assert!(db.receiver_job(accepted.job_id()).unwrap().is_some());
}
#[test]
fn receiver_claim_selects_the_oldest_ready_job() {
    let db = Db::open_in_memory().expect("receiver state");
    let newer = receiver_job(None, 200);
    let older = receiver_job(None, 100);
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    db.accept_receiver_job(&newer, &identity)
        .expect("accept newer job");
    let expected = db
        .accept_receiver_job(&older, &identity)
        .expect("accept older job");

    let claimed = db
        .claim_next_receiver_job("worker-a", 1_000, 1_100)
        .expect("claim oldest job")
        .expect("claim available");

    assert_eq!(claimed.job_id(), expected.job_id());
}

#[test]
fn a_queued_restart_prevents_an_older_job_from_being_claimed() {
    let db = Db::open_in_memory().expect("receiver state");
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let older = receiver_job(Some("older-before-restart"), 100);
    let mut restart = receiver_job(Some("restart-control"), 200);
    restart.prompt = " /ReStArT\n".to_owned();
    let older_acceptance = db
        .accept_receiver_job(&older, &identity)
        .expect("accept older backlog");
    let restart_acceptance = db
        .accept_receiver_job(&restart, &identity)
        .expect("accept restart control");

    assert!(
        db.claim_next_receiver_run("remote-owner", 1_000, 1_100)
            .expect("make atomic restart-or-claim decision")
            .is_none(),
        "a ready restart must be processed before ordinary backlog can be claimed"
    );
    assert_eq!(
        db.receiver_job(older_acceptance.job_id())
            .unwrap()
            .unwrap()
            .state(),
        ReceiverJobState::Queued
    );
    assert_eq!(
        db.receiver_job(restart_acceptance.job_id())
            .unwrap()
            .unwrap()
            .state(),
        ReceiverJobState::Queued
    );
}

#[test]
fn only_the_live_claim_owner_can_renew_or_advance_a_job() {
    let db = Db::open_in_memory().expect("receiver state");
    let job = receiver_job(None, 100);
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let accepted = db
        .accept_receiver_job(&job, &identity)
        .expect("accept receiver job");
    register_observation_session(&db, accepted.conversation_id(), &job, "instance-a", "session-a");
    db.claim_next_receiver_job("worker-a", 1_000, 1_100)
        .expect("claim job")
        .expect("claim available");

    assert!(!db
        .renew_receiver_claim(accepted.job_id(), "worker-b", 1_050, 1_200)
        .expect("reject foreign renewal"));
    assert!(!db
        .transition_receiver_job(
            accepted.job_id(),
            "worker-b",
            ReceiverJobState::Claimed,
            ReceiverJobState::Launching,
            1_050,
        )
        .expect("reject foreign transition"));
    assert!(db
        .renew_receiver_claim(accepted.job_id(), "worker-a", 1_050, 1_200)
        .expect("renew owned claim"));

    assert!(db
        .transition_receiver_job(
            accepted.job_id(),
            "worker-a",
            ReceiverJobState::Claimed,
            ReceiverJobState::Launching,
            1_060,
        )
        .expect("prepare owned launch"));
    let token = db
        .receiver_job(accepted.job_id())
        .expect("load launch")
        .expect("job")
        .token();
    assert!(db
        .commit_receiver_job_launch(accepted.job_id(), "worker-a", &launch_observation(token, "instance-a", "session-a", 1_060))
        .expect("commit owned launch"));
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
                1_061,
            ),
        )
        .expect("record accepted evidence"));
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
                1_062,
            ),
        )
        .expect("record progressing evidence"));
    for (expected, next) in [
        (ReceiverJobState::Processing, ReceiverJobState::AnswerReady),
        (ReceiverJobState::AnswerReady, ReceiverJobState::Delivering),
        (ReceiverJobState::Delivering, ReceiverJobState::Done),
    ] {
        assert!(db
            .transition_receiver_job(accepted.job_id(), "worker-a", expected, next, 1_060)
            .expect("advance owned job"));
        assert_eq!(
            db.receiver_job(accepted.job_id())
                .expect("load transitioned job")
                .expect("job remains durable")
                .state(),
            next
        );
    }
}

#[test]
fn retry_state_and_metadata_survive_database_reopen() {
    let temporary = tempfile::tempdir().expect("temporary receiver state");
    let path = temporary.path().join("state.db");
    let job = receiver_job(None, 100);
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let job_id = {
        let db = Db::open_path_with_legacy_identity(
            &path,
            &receiver_workspace_id().to_string(),
            receiver_user_id().as_str(),
        )
        .expect("open receiver state");
        let accepted = db
            .accept_receiver_job(&job, &identity)
            .expect("accept receiver job");
        db.claim_next_receiver_job("worker-a", 1_000, 1_100)
            .expect("claim job")
            .expect("claim available");
        assert!(db
            .record_receiver_retry(
                accepted.job_id(),
                "worker-a",
                ReceiverJobState::Claimed,
                1_050,
                2_000,
                "launch-unavailable",
            )
            .expect("record retry"));
        accepted.job_id()
    };
    let reopened = Db::open_path_with_legacy_identity(
        &path,
        &receiver_workspace_id().to_string(),
        receiver_user_id().as_str(),
    )
    .expect("reopen receiver state");

    let persisted = reopened
        .receiver_job(job_id)
        .expect("load retrying job")
        .expect("job remains durable");
    assert_eq!(persisted.state(), ReceiverJobState::Retrying);
    assert_eq!(persisted.retry_count(), 1);
    assert_eq!(persisted.retry_at_unix_ms(), Some(2_000));
    assert_eq!(persisted.last_error(), Some("launch-unavailable"));
}
