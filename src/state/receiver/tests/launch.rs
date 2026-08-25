#[test]
fn fifo_receiver_run_claim_loads_the_immutable_job_and_conversation_without_deleting_it() {
    let db = Db::open_in_memory().expect("receiver state");
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let newer = receiver_job(Some("newer"), 200);
    let older = receiver_job(Some("older"), 100);
    db.accept_receiver_job(&newer, &identity)
        .expect("accept newer job");
    let accepted = db
        .accept_receiver_job(&older, &identity)
        .expect("accept older job");

    let claimed = db
        .claim_next_receiver_run("remote-owner", 1_000, 1_100)
        .expect("claim receiver run")
        .expect("ready receiver run");

    assert_eq!(claimed.claim().job_id(), accepted.job_id());
    assert_eq!(claimed.claim().owner(), "remote-owner");
    assert_eq!(claimed.job().id(), accepted.job_id());
    assert_eq!(claimed.job().inbound(), &older);
    assert_eq!(claimed.conversation().id(), accepted.conversation_id());
    assert_eq!(claimed.conversation().identity(), &identity);
    assert!(db.receiver_job(accepted.job_id()).unwrap().is_some());
    assert!(
        db.claim_next_receiver_run("second-owner", 1_050, 1_200)
            .expect("poll while first claim is live")
            .is_none(),
        "one workspace must have at most one live receiver claim"
    );
}

#[test]
fn launch_preparation_requires_the_exact_live_owner_and_launch_eligible_state() {
    let db = Db::open_in_memory().expect("receiver state");
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let job = receiver_job(None, 100);
    let accepted = db
        .accept_receiver_job(&job, &identity)
        .expect("accept receiver job");
    db.claim_next_receiver_run("remote-owner", 1_000, 1_100)
        .expect("claim receiver run")
        .expect("ready receiver run");

    assert!(!db
        .prepare_receiver_job_launch(accepted.job_id(), "stale-owner", 1_050)
        .expect("reject stale owner"));
    assert!(!db
        .prepare_receiver_job_launch(accepted.job_id(), "remote-owner", 1_100)
        .expect("reject expired owner"));
    assert_eq!(
        db.receiver_job(accepted.job_id())
            .expect("load rejected launch")
            .expect("durable job")
            .state(),
        ReceiverJobState::Claimed
    );

    assert!(db
        .prepare_receiver_job_launch(accepted.job_id(), "remote-owner", 1_050)
        .expect("prepare exact launch"));
    assert_eq!(
        db.receiver_job(accepted.job_id())
            .expect("load prepared launch")
            .expect("durable job")
            .state(),
        ReceiverJobState::Launching
    );
}

#[test]
fn launched_observations_require_the_exact_owner_instance_session_and_revision() {
    let db = Db::open_in_memory().expect("receiver state");
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let accepted = db
        .accept_receiver_job(&receiver_job(None, 100), &identity)
        .expect("accept receiver job");
    db.claim_next_receiver_run("owner", 1_000, 2_000)
        .expect("claim receiver job")
        .expect("receiver claim");
    db.prepare_receiver_job_launch(accepted.job_id(), "owner", 1_100)
        .expect("prepare receiver launch");
    let token = db
        .receiver_job(accepted.job_id())
        .expect("load receiver job")
        .expect("receiver job")
        .token();

    assert!(db
        .commit_receiver_job_launch(accepted.job_id(), "owner", &launch_observation(token, "instance-a", "session-a", 1_200))
        .expect("commit launched evidence"));
    assert!(!db
        .apply_receiver_observation(accepted.job_id(), "owner", &observation(token, "instance-b", "session-a", ReceiverObservationPhase::Accepted, 1, 1_300))
        .expect("reject stale instance"));
    assert!(db
        .apply_receiver_observation(accepted.job_id(), "owner", &observation(token, "instance-a", "session-a", ReceiverObservationPhase::Accepted, 1, 1_300))
        .expect("apply accepted evidence"));
    assert!(db
        .apply_receiver_observation(accepted.job_id(), "owner", &observation(token, "instance-a", "session-a", ReceiverObservationPhase::Progressing, 2, 1_400))
        .expect("apply progressing evidence"));

    let job = db
        .receiver_job(accepted.job_id())
        .expect("load observed receiver job")
        .expect("receiver job");
    assert_eq!(job.state(), ReceiverJobState::Processing);
    assert_eq!(job.launched_at_unix_ms(), Some(1_200));
    assert_eq!(job.accepted_at_unix_ms(), Some(1_300));
    assert_eq!(job.progressing_at_unix_ms(), Some(1_400));
    assert_eq!(job.observation_revision(), 2);
}

#[test]
fn only_a_due_pre_acceptance_retry_can_prepare_another_launch() {
    for (retry_from, eligible) in [
        (ReceiverJobState::Claimed, true),
        (ReceiverJobState::Launching, true),
        (ReceiverJobState::Accepted, false),
        (ReceiverJobState::Processing, false),
        (ReceiverJobState::Delivering, false),
    ] {
        let db = Db::open_in_memory().expect("receiver state");
        let identity =
            ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
        let job = receiver_job(None, 100);
        let accepted = db
            .accept_receiver_job(&job, &identity)
            .expect("accept receiver job");
        db.conn
            .execute(
                "UPDATE receiver_jobs
                 SET state = 'retrying', retry_from_state = ?1, retry_count = 1,
                     retry_at_unix_ms = 2_000
                 WHERE job_id = ?2",
                rusqlite::params![retry_from.as_str(), accepted.job_id().to_string()],
            )
            .expect("seed receiver retry");
        db.claim_next_receiver_run("remote-owner", 2_000, 2_100)
            .expect("claim due retry")
            .expect("due retry");

        assert_eq!(
            db.prepare_receiver_job_launch(accepted.job_id(), "remote-owner", 2_050)
                .expect("classify retry launch"),
            eligible,
            "retry from {retry_from:?}"
        );
        assert_eq!(
            db.receiver_job(accepted.job_id())
                .expect("load retry")
                .expect("durable retry")
                .state(),
            if eligible {
                ReceiverJobState::Launching
            } else {
                ReceiverJobState::Retrying
            }
        );
    }
}

#[test]
fn generic_transition_cannot_launch_retries_from_progressed_states() {
    for retry_from in [
        ReceiverJobState::Accepted,
        ReceiverJobState::Processing,
        ReceiverJobState::Delivering,
    ] {
        let db = Db::open_in_memory().expect("receiver state");
        let identity =
            ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
        let accepted = db
            .accept_receiver_job(&receiver_job(None, 100), &identity)
            .expect("accept receiver job");
        db.conn
            .execute(
                "UPDATE receiver_jobs
                 SET state = 'retrying', retry_from_state = ?1, retry_count = 1,
                     retry_at_unix_ms = 2_000
                 WHERE job_id = ?2",
                rusqlite::params![retry_from.as_str(), accepted.job_id().to_string()],
            )
            .expect("seed progressed receiver retry");
        db.claim_next_receiver_run("remote-owner", 2_000, 2_100)
            .expect("claim due retry")
            .expect("due retry");

        assert!(!db
            .transition_receiver_job(
                accepted.job_id(),
                "remote-owner",
                ReceiverJobState::Retrying,
                ReceiverJobState::Launching,
                2_050,
            )
            .expect("reject generic progressed launch"));
        let persisted = db
            .receiver_job(accepted.job_id())
            .expect("load retry")
            .expect("durable retry");
        assert_eq!(persisted.state(), ReceiverJobState::Retrying);
        assert_eq!(persisted.retry_from_state(), Some(retry_from));
        assert_eq!(persisted.retry_at_unix_ms(), Some(2_000));
    }
}

#[test]
fn pre_acceptance_launch_failures_schedule_only_a_bounded_number_of_durable_retries() {
    let db = Db::open_in_memory().expect("receiver state");
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let job = receiver_job(None, 100);
    let accepted = db
        .accept_receiver_job(&job, &identity)
        .expect("accept receiver job");

    for attempt in 1..=MAX_RECEIVER_LAUNCH_ATTEMPTS {
        let now = u64::from(attempt) * 1_000;
        db.claim_next_receiver_run("remote-owner", now, now + 500)
            .expect("claim launch attempt")
            .expect("launch attempt is ready");
        assert!(db
            .prepare_receiver_job_launch(accepted.job_id(), "remote-owner", now + 10)
            .expect("prepare launch attempt"));
        let outcome = db
            .record_receiver_launch_retry(
                accepted.job_id(),
                "remote-owner",
                now + 20,
                now + 1_000,
                ReceiverLaunchFailure::Spawn,
            )
            .expect("record launch failure")
            .expect("exact owner records failure");
        let persisted = db
            .receiver_job(accepted.job_id())
            .expect("load failed launch")
            .expect("job remains durable");
        assert_eq!(persisted.retry_count(), attempt);
        assert_eq!(persisted.last_error(), Some("launch-spawn"));
        if attempt < MAX_RECEIVER_LAUNCH_ATTEMPTS {
            assert_eq!(outcome, ReceiverLaunchRetryOutcome::Scheduled);
            assert_eq!(persisted.state(), ReceiverJobState::Retrying);
            assert_eq!(
                persisted.retry_from_state(),
                Some(ReceiverJobState::Launching)
            );
            assert_eq!(persisted.retry_at_unix_ms(), Some(now + 1_000));
        } else {
            assert_eq!(outcome, ReceiverLaunchRetryOutcome::Exhausted);
            assert_eq!(persisted.state(), ReceiverJobState::Failed);
            assert_eq!(persisted.retry_at_unix_ms(), None);
        }
    }
}

#[test]
fn planning_and_registration_failures_can_retry_again_before_launch_preparation() {
    for failure in [
        ReceiverLaunchFailure::Planning,
        ReceiverLaunchFailure::Registration,
    ] {
        let db = Db::open_in_memory().expect("receiver state");
        let identity =
            ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
        let job = receiver_job(None, 100);
        let accepted = db
            .accept_receiver_job(&job, &identity)
            .expect("accept receiver job");
        db.claim_next_receiver_run("remote-owner", 1_000, 1_500)
            .expect("claim first launch attempt")
            .expect("first launch attempt");
        db.record_receiver_launch_retry(
            accepted.job_id(),
            "remote-owner",
            1_010,
            2_000,
            failure,
        )
        .expect("record first launch failure")
        .expect("first failure owns the claim");
        db.claim_next_receiver_run("remote-owner", 2_000, 2_500)
            .expect("claim due launch retry")
            .expect("due launch retry");

        assert_eq!(
            db.record_receiver_launch_retry(
                accepted.job_id(),
                "remote-owner",
                2_010,
                3_000,
                failure,
            )
            .expect("record repeated pre-preparation failure"),
            Some(ReceiverLaunchRetryOutcome::Scheduled),
            "{failure:?} must retain retry ownership before launch preparation"
        );
    }
}

#[test]
fn launch_failure_retry_rejects_stale_owners_and_progressed_states() {
    for failure in ReceiverLaunchFailure::ALL {
        let db = Db::open_in_memory().expect("receiver state");
        let identity =
            ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
        let job = receiver_job(None, 100);
        let accepted = db
            .accept_receiver_job(&job, &identity)
            .expect("accept receiver job");
        db.claim_next_receiver_run("remote-owner", 1_000, 1_100)
            .expect("claim receiver job")
            .expect("ready job");

        assert!(db
            .record_receiver_launch_retry(
                accepted.job_id(),
                "stale-owner",
                1_050,
                2_000,
                failure,
            )
            .expect("reject stale owner")
            .is_none());
        db.conn
            .execute(
                "UPDATE receiver_jobs SET state = 'accepted' WHERE job_id = ?1",
                [accepted.job_id().to_string()],
            )
            .expect("seed progressed state");
        assert!(db
            .record_receiver_launch_retry(
                accepted.job_id(),
                "remote-owner",
                1_050,
                2_000,
                failure,
            )
            .expect("reject progressed state")
            .is_none());
        assert_eq!(
            db.receiver_job(accepted.job_id())
                .unwrap()
                .unwrap()
                .state(),
            ReceiverJobState::Accepted
        );
    }
}
