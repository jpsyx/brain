#[test]
fn answer_cleanup_releases_only_its_exact_session_then_finishes_after_artifacts() {
    let fixture = super::binding::completion_fixture(ReceiverJobState::Processing);
    fixture
        .db
        .complete_receiver_job_with_binding(&fixture.request())
        .expect("record exact answer")
        .expect("exact answer owner");
    let cleanup = fixture
        .db
        .receiver_answer_cleanup(fixture.job_id)
        .expect("load answer cleanup")
        .expect("pending answer cleanup");

    assert!(!cleanup.session_released());
    assert!(!cleanup.artifacts_removed());
    assert!(
        !fixture
            .db
            .finish_receiver_answer_cleanup(&cleanup)
            .expect("unfinished cleanup cannot finish")
    );
    assert!(
        fixture
            .db
            .acknowledge_receiver_answer_controller_shutdown(
                cleanup.job_id(),
                cleanup.token(),
                cleanup.instance(),
                42,
                1_550,
            )
            .expect("acknowledge exact controller shutdown")
    );
    let cleanup = fixture
        .db
        .receiver_answer_cleanup(fixture.job_id)
        .expect("reload acknowledged cleanup")
        .expect("acknowledged cleanup remains pending");
    assert!(
        fixture
            .db
            .release_receiver_answer_cleanup_session(&cleanup, 1_600)
            .expect("release exact answer session")
    );
    let after_release = fixture
        .db
        .receiver_answer_cleanup(fixture.job_id)
        .expect("reload answer cleanup")
        .expect("cleanup still pending artifacts");
    assert!(after_release.session_released());
    assert!(!after_release.artifacts_removed());
    assert_eq!(
        fixture
            .db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM receiver_session_registrations",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("remaining receiver registrations"),
        0
    );
    assert!(
        fixture
            .db
            .mark_receiver_answer_artifacts_removed(&after_release, 1_700)
            .expect("acknowledge exact artifacts")
    );
    let complete = fixture
        .db
        .receiver_answer_cleanup(fixture.job_id)
        .expect("reload completed local effects")
        .expect("cleanup pending final handoff");
    assert!(complete.session_released());
    assert!(complete.artifacts_removed());
    assert!(
        fixture
            .db
            .finish_receiver_answer_cleanup(&complete)
            .expect("finish exact answer cleanup")
    );
    assert!(
        fixture
            .db
            .receiver_answer_cleanup(fixture.job_id)
            .expect("reload finished cleanup")
            .is_none()
    );
}

#[test]
fn live_controller_fences_answer_cleanup_until_exact_shutdown_acknowledgement() {
    let fixture = super::binding::completion_fixture(ReceiverJobState::Processing);
    fixture
        .db
        .complete_receiver_job_with_binding(&fixture.request())
        .expect("record exact answer")
        .expect("exact answer owner");

    assert!(
        fixture
            .db
            .next_receiver_answer_cleanup()
            .expect("inspect live cleanup fence")
            .is_none(),
        "a live unacknowledged controller must retain cleanup authority"
    );
    let cleanup = fixture
        .db
        .receiver_answer_cleanup(fixture.job_id)
        .expect("load exact cleanup")
        .expect("pending exact cleanup");
    assert!(
        fixture
            .db
            .acknowledge_receiver_answer_controller_shutdown(
                cleanup.job_id(),
                cleanup.token(),
                cleanup.instance(),
                42,
                1_600,
            )
            .expect("acknowledge exact controller shutdown")
    );
    assert!(
        fixture
            .db
            .next_receiver_answer_cleanup()
            .expect("load acknowledged cleanup")
            .is_some()
    );
}

#[test]
fn dead_controller_makes_unacknowledged_answer_cleanup_recoverable() {
    let fixture = super::binding::completion_fixture(ReceiverJobState::Processing);
    fixture
        .db
        .complete_receiver_job_with_binding(&fixture.request())
        .expect("record exact answer")
        .expect("exact answer owner");
    let super::binding::CompletionFixture { db, job_id, .. } = fixture;
    let db = db.with_pid_alive(|_| false);
    db.reap_dead_locks().expect("reap the dead controller lock");

    let cleanup = db
        .next_receiver_answer_cleanup()
        .expect("load dead-owner cleanup")
        .expect("dead controller permits takeover");

    assert!(
        cleanup.job_id() == job_id,
        "dead-owner cleanup selected the wrong job"
    );
}

#[test]
fn stale_same_scope_pid_row_does_not_authorize_answer_cleanup_takeover() {
    let fixture = super::binding::completion_fixture(ReceiverJobState::Processing);
    fixture
        .db
        .complete_receiver_job_with_binding(&fixture.request())
        .expect("record exact answer")
        .expect("exact answer owner");
    fixture
        .db
        .conn
        .execute(
            "INSERT INTO brain_sessions
               (agent_kind, agent_session_id, brain_instance_id, locked_pid, source,
                created_at, last_active_at, workspace_id, actor_id, channel, completion_status)
             SELECT agent_kind, 'reused-pid-session', 'replacement-brain-instance',
                    locked_pid, 'fresh', created_at, last_active_at,
                    workspace_id, actor_id, channel, 'active'
             FROM brain_sessions WHERE brain_instance_id = 'completion-instance'",
            [],
        )
        .expect("stage a different Brain incarnation with the reused PID");

    assert!(
        fixture
        .db
        .next_receiver_answer_cleanup()
        .expect("inspect stale-PID cleanup fence")
        .is_none(),
        "another row with the same PID is not exact child-exit proof"
    );
}

#[test]
fn main_session_with_the_same_pid_does_not_authorize_receiver_takeover() {
    let fixture = super::binding::completion_fixture(ReceiverJobState::Processing);
    fixture
        .db
        .complete_receiver_job_with_binding(&fixture.request())
        .expect("record exact answer")
        .expect("exact answer owner");
    fixture
        .db
        .conn
        .execute(
            "INSERT INTO brain_sessions
               (agent_kind, agent_session_id, brain_instance_id, locked_pid, source,
                created_at, last_active_at, workspace_id, actor_id, channel, completion_status)
             SELECT agent_kind, 'interactive-main-session', 'interactive-main-instance',
                    locked_pid, 'fresh', created_at, last_active_at,
                    workspace_id, 'local-user', 'interactive', 'active'
             FROM brain_sessions WHERE brain_instance_id = 'completion-instance'",
            [],
        )
        .expect("stage a realistic main-session row with the same process PID");

    assert!(
        fixture
            .db
            .next_receiver_answer_cleanup()
            .expect("inspect same-PID main-session fence")
            .is_none(),
        "the process PID of a main panel is not receiver child-exit proof"
    );
}

#[test]
fn later_answer_commits_while_prior_same_instance_cleanup_remains_pending() {
    let first = super::binding::completion_fixture(ReceiverJobState::Processing);
    first
        .db
        .complete_receiver_job_with_binding(&first.request())
        .expect("record first exact answer")
        .expect("first exact answer owner");
    let first_cleanup = first
        .db
        .receiver_answer_cleanup(first.job_id)
        .expect("load first cleanup")
        .expect("first cleanup authority");
    assert!(
        first
            .db
            .acknowledge_receiver_answer_controller_shutdown(
                first_cleanup.job_id(),
                first_cleanup.token(),
                first_cleanup.instance(),
                42,
                1_600,
            )
            .expect("acknowledge first child exit")
    );
    assert!(
        first
            .db
            .release_receiver_answer_cleanup_session(&first_cleanup, 1_700)
            .expect("release first exact session")
    );

    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let second_job = receiver_job(Some("later-answer"), 200);
    let second = first
        .db
        .accept_receiver_job(&second_job, &identity)
        .expect("accept later job");
    let registration = first
        .db
        .claim_receiver_session(
            second.conversation_id(),
            &first.completed_session,
            first.registration.instance(),
            42,
            first.registration.scope(),
        )
        .expect("claim prior native session")
        .expect("same instance reclaims native session");
    let claim = first
        .db
        .claim_next_receiver_run("later-owner", 2_000, 3_000)
        .expect("claim later job")
        .expect("later job is available");
    assert_eq!(claim.job().id(), second.job_id());
    assert!(
        first
            .db
            .prepare_receiver_job_launch(second.job_id(), "later-owner", 2_100)
            .expect("prepare later launch")
    );
    let token = first
        .db
        .receiver_job(second.job_id())
        .expect("load later job")
        .expect("durable later job")
        .token();
    assert!(
        first
            .db
            .commit_receiver_job_launch(
                second.job_id(),
                "later-owner",
                &launch_observation(
                    token,
                    registration.instance(),
                    first.completed_session.as_str(),
                    2_200,
                ),
            )
            .expect("commit later launch")
    );
    let request = ReceiverCompletionRequest {
        job_id: second.job_id(),
        token,
        owner: "later-owner",
        registration: &registration,
        completed_session: &first.completed_session,
        answer: "later exact assistant answer",
        observed_at_unix_ms: 2_300,
        authorized_at_unix_ms: 2_300,
    };

    assert!(
        first
            .db
            .complete_receiver_job_with_binding(&request)
            .expect("a prior cleanup must not reject the later answer")
            .is_some()
    );
    let cleanups: i64 = first
        .db
        .conn
        .query_row("SELECT COUNT(*) FROM receiver_answer_cleanups", [], |row| {
            row.get(0)
        })
        .expect("count independent cleanup rows");
    assert_eq!(cleanups, 2);
}
