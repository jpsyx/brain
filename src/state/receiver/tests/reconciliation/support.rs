struct LaunchedRunFixture {
    db: Db,
    job_id: ReceiverJobId,
}

fn launched_run(provider_id: &str, claim_expires_at_unix_ms: u64) -> LaunchedRunFixture {
    let db = Db::open_in_memory().expect("receiver state");
    let inbound = receiver_job(Some(provider_id), 100);
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let accepted = db
        .accept_receiver_job(&inbound, &identity)
        .expect("accept receiver job");
    let job_id = accepted.job_id();
    db.claim_next_receiver_run("launch-instance", 1_000, claim_expires_at_unix_ms)
        .expect("claim receiver run")
        .expect("receiver run");
    assert!(
        db.prepare_receiver_job_launch(job_id, "launch-instance", 1_100)
            .expect("prepare receiver launch")
    );
    register_observation_session(
        &db,
        accepted.conversation_id(),
        &inbound,
        "launch-instance",
        "native-session",
    );
    let token = db
        .receiver_job(job_id)
        .expect("load receiver job")
        .expect("receiver job")
        .token();
    assert!(
        db.commit_receiver_job_launch(
            job_id,
            "launch-instance",
            &launch_observation(token, "launch-instance", "native-session", 1_200),
        )
        .expect("commit receiver launch")
    );
    LaunchedRunFixture { db, job_id }
}

struct AcceptedRunFixture {
    db: Db,
    inbound: crate::server::receiver::InboundJob,
    job_id: ReceiverJobId,
    ordinary: ReceiverJob,
}

fn accepted_run(provider_id: &str) -> AcceptedRunFixture {
    accepted_run_in(
        Db::open_in_memory().expect("receiver state"),
        provider_id,
    )
}

fn accepted_run_in(db: Db, provider_id: &str) -> AcceptedRunFixture {
    let inbound = receiver_job(Some(provider_id), 100);
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let accepted = db
        .accept_receiver_job(&inbound, &identity)
        .expect("accept receiver job");
    let job_id = accepted.job_id();
    db.claim_next_receiver_run("ordinary-owner", 1_000, 2_000)
        .expect("claim ordinary run")
        .expect("ordinary run");
    assert!(
        db.prepare_receiver_job_launch(job_id, "ordinary-owner", 1_100)
            .expect("prepare ordinary launch")
    );
    register_observation_session(
        &db,
        accepted.conversation_id(),
        &inbound,
        "ordinary-instance",
        "native-session",
    );
    let token = db
        .receiver_job(job_id)
        .expect("load ordinary job")
        .expect("ordinary job")
        .token();
    assert!(
        db.commit_receiver_job_launch(
            job_id,
            "ordinary-owner",
            &launch_observation(token, "ordinary-instance", "native-session", 1_200),
        )
        .expect("commit ordinary launch")
    );
    assert!(
        db.apply_receiver_observation(
            job_id,
            "ordinary-owner",
            &observation(
                token,
                "ordinary-instance",
                "native-session",
                ReceiverNonterminalObservationPhase::Accepted,
                1,
                1_300,
            ),
        )
        .expect("commit ordinary acceptance")
    );
    assert!(
        db.apply_receiver_observation(
            job_id,
            "ordinary-owner",
            &observation(
                token,
                "ordinary-instance",
                "native-session",
                ReceiverNonterminalObservationPhase::Progressing,
                2,
                1_400,
            ),
        )
        .expect("commit ordinary progress")
    );
    let ordinary = db
        .receiver_job(job_id)
        .expect("load stalled ordinary job")
        .expect("stalled ordinary job");
    AcceptedRunFixture {
        db,
        inbound,
        job_id,
        ordinary,
    }
}

fn acknowledge_accepted_run_cleanup(fixture: &AcceptedRunFixture, now_unix_ms: u64) {
    assert!(
        fixture
            .db
            .acknowledge_receiver_recovery_cleanup(
                fixture.job_id,
                fixture.ordinary.token(),
                "ordinary-instance",
                "native-session",
                now_unix_ms,
            )
            .expect("acknowledge accepted-run cleanup")
    );
}
