#[test]
fn portable_transcript_appends_exact_markdown_escaped_turn_once() {
    let prior = "# Prior\n\nExisting context without a final newline";
    let inbound = "User text\n```\n## forged heading\n<script>private</script>";
    let answer = "Assistant text\n````\n## another heading\n<answer>exact</answer>\n";

    let appended = render_receiver_transcript(prior, inbound, answer);
    let duplicate = render_receiver_transcript(&appended, inbound, answer);

    assert!(appended.starts_with(prior));
    assert!(appended.contains(inbound));
    assert!(appended.contains(answer));
    assert_eq!(appended.matches("## Authenticated user").count(), 1);
    assert_eq!(appended.matches("## Assistant").count(), 1);
    assert_ne!(duplicate, appended, "the pure renderer exposes append semantics");
    assert!(
        receiver_transcript_has_exact_turn(&appended, inbound, answer),
        "the stored transcript must recognize an exact duplicate turn"
    );
    assert!(!receiver_transcript_has_exact_turn(
        &appended,
        inbound,
        "conflicting answer"
    ));
}

#[test]
fn exact_completion_atomically_records_answer_ready_transcript_binding_and_outbox_once() {
    let fixture = super::binding::completion_fixture(ReceiverJobState::Processing);
    fixture
        .db
        .update_receiver_conversation(
            fixture.registration.conversation_id(),
            "# Prior\n\nDurable context",
            None,
            1_450,
        )
        .expect("seed prior transcript");
    let request = fixture.request();

    let first = fixture
        .db
        .complete_receiver_job_with_binding(&request)
        .expect("record exact answer")
        .expect("exact answer owner");
    let first_transcript = fixture
        .db
        .receiver_conversation(fixture.registration.conversation_id())
        .expect("load conversation")
        .expect("durable conversation")
        .transcript_markdown()
        .to_owned();
    let second = fixture
        .db
        .complete_receiver_job_with_binding(&request)
        .expect("replay exact answer")
        .expect("existing exact answer");

    assert!(first.newly_recorded());
    assert!(!second.newly_recorded());
    assert_eq!(first.delivery_id(), second.delivery_id());
    assert_eq!(
        fixture
            .db
            .receiver_job(fixture.job_id)
            .expect("load answer-ready job")
            .expect("durable job")
            .state(),
        ReceiverJobState::AnswerReady
    );
    assert_eq!(
        fixture
            .db
            .receiver_conversation(fixture.registration.conversation_id())
            .expect("reload conversation")
            .expect("durable conversation")
            .transcript_markdown(),
        first_transcript
    );
    assert_eq!(first_transcript.matches("## Authenticated user").count(), 1);
    assert_eq!(first_transcript.matches("## Assistant").count(), 1);
    let (delivery_count, delivery_state, claim_owner): (i64, String, Option<String>) = fixture
        .db
        .conn
        .query_row(
            "SELECT COUNT(*), MIN(delivery.state), MIN(job.claim_owner)
             FROM receiver_deliveries AS delivery
             JOIN receiver_jobs AS job ON job.job_id = delivery.job_id
             WHERE delivery.job_id = ?1 AND delivery.response_kind = 'final-answer'",
            [fixture.job_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("load answer outbox");
    assert_eq!(delivery_count, 1);
    assert_eq!(delivery_state, "ready");
    assert_eq!(claim_owner, None);
    let cleanup: (i64, String, String, i64, i64) = fixture
        .db
        .conn
        .query_row(
            "SELECT COUNT(*), MIN(brain_instance_id), MIN(registered_session_id),
                    MIN(session_released), MIN(artifacts_removed)
             FROM receiver_answer_cleanups WHERE job_id = ?1",
            [fixture.job_id.to_string()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .expect("load exact post-commit cleanup");
    assert_eq!(cleanup.0, 1);
    assert_eq!(cleanup.1, fixture.registration.instance());
    assert_eq!(
        cleanup.2,
        fixture.registration.registered_session().as_str()
    );
    assert_eq!((cleanup.3, cleanup.4), (0, 0));
}

#[test]
fn completion_uses_the_sender_frozen_with_the_authenticated_inbound_job() {
    let fixture = super::binding::completion_fixture(ReceiverJobState::Processing);
    let request = fixture.request();

    fixture
        .db
        .complete_receiver_job_with_binding(&request)
        .expect("record answer after machine sender changed")
        .expect("exact answer owner");
    let claim = fixture
        .db
        .claim_next_receiver_delivery("delivery-owner", 2_000, 32_000)
        .expect("claim frozen delivery")
        .expect("frozen delivery exists");

    assert!(
        claim
            .envelope()
            .sms()
            .is_some_and(|sms| sms.sender() == "+12125550100"),
        "delivery did not use the sender frozen at inbound acceptance"
    );
}

#[test]
fn email_completion_without_an_authorized_recipient_is_terminal_and_restart_idempotent() {
    let temporary = tempfile::tempdir().expect("temporary receiver state");
    let path = temporary.path().join("state.db");
    let workspace = receiver_workspace_id().to_string();
    let actor = receiver_user_id();
    let fixture = super::binding::email_completion_fixture_in(
        Db::open_path_with_legacy_identity(&path, &workspace, actor.as_str())
            .expect("file-backed receiver state"),
        ReceiverJobState::Processing,
    );
    let first = fixture
        .db
        .complete_receiver_job_with_binding(&fixture.request())
        .expect("record terminal authorization outcome")
        .expect("exact terminal outcome");
    let row: (String, Option<String>, i64) = fixture
        .db
        .conn
        .query_row(
            "SELECT delivery.state, delivery.error_category,
                    (SELECT COUNT(*) FROM receiver_answer_cleanups AS cleanup
                     WHERE cleanup.job_id = delivery.job_id)
             FROM receiver_deliveries AS delivery WHERE delivery.job_id = ?1",
            [fixture.job_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("load terminal authorization outcome");
    assert!(row.0 == "failed", "authorization outcome was sendable");
    assert!(
        row.1.as_deref() == Some("authorization"),
        "terminal outcome had the wrong content-free category"
    );
    assert!(row.2 == 1, "answer cleanup authority was not persisted");
    assert!(
        fixture
            .db
            .receiver_job(fixture.job_id)
            .expect("load terminal job")
            .is_some_and(|job| job.state() == ReceiverJobState::Failed),
        "authorization failure did not release the agent lane"
    );
    assert!(
        fixture
            .db
            .claim_next_receiver_delivery("delivery-owner", 2_000, 32_000)
            .expect("inspect delivery lane")
            .is_none(),
        "authorization failure entered the provider delivery lane"
    );
    let transcript = fixture
        .db
        .receiver_conversation(fixture.registration.conversation_id())
        .expect("load terminal conversation")
        .expect("terminal conversation")
        .transcript_markdown()
        .to_owned();
    assert!(
        receiver_transcript_has_exact_turn(
            &transcript,
            "Remember the durable receiver job",
            "exact assistant answer",
        ),
        "terminal authorization did not advance the portable transcript"
    );

    let job_id = fixture.job_id;
    let token = fixture.token;
    let registration = fixture.registration.clone();
    let completed_session = fixture.completed_session.clone();
    drop(fixture);
    let reopened = Db::open_path_with_legacy_identity(&path, &workspace, actor.as_str())
        .expect("reopen terminal receiver state");
    let replay = reopened
        .complete_receiver_job_with_binding(&ReceiverCompletionRequest {
            job_id,
            token,
            owner: "owner",
            registration: &registration,
            completed_session: &completed_session,
            answer: "exact assistant answer",
            observed_at_unix_ms: 1_500,
            authorized_at_unix_ms: 1_500,
        })
        .expect("replay terminal authorization outcome")
        .expect("existing terminal outcome");
    assert!(!first.delivery_id().to_string().is_empty());
    assert!(!replay.newly_recorded());
}

#[test]
fn completion_terminalizes_a_legacy_job_without_a_frozen_response_sender() {
    let fixture = super::binding::completion_fixture(ReceiverJobState::Processing);
    fixture
        .db
        .conn
        .execute(
            "UPDATE receiver_jobs SET response_sender = NULL WHERE job_id = ?1",
            [fixture.job_id.to_string()],
        )
        .expect("stage legacy accepted inbound job");

    fixture
        .db
        .complete_receiver_job_with_binding(&fixture.request())
        .expect("terminalize missing frozen sender")
        .expect("exact terminal outcome");
    let row: (String, Option<String>) = fixture
        .db
        .conn
        .query_row(
            "SELECT state, error_category FROM receiver_deliveries WHERE job_id = ?1",
            [fixture.job_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("load legacy terminal outcome");
    assert!(row.0 == "failed", "legacy sender outcome was sendable");
    assert!(
        row.1.as_deref() == Some("invalid-request"),
        "legacy sender outcome had the wrong content-free category"
    );
    assert!(
        fixture
            .db
            .claim_next_receiver_delivery("delivery-owner", 2_000, 32_000)
            .expect("inspect delivery lane")
            .is_none(),
        "legacy sender outcome entered the provider delivery lane"
    );
}

#[test]
fn completion_terminalizes_every_invalid_persisted_response_sender_shape() {
    let cases = [
        (
            super::binding::completion_fixture(ReceiverJobState::Processing),
            "(212) 555-0100",
        ),
        (
            super::binding::completion_fixture(ReceiverJobState::Processing),
            "invalid-sms-sender",
        ),
        (
            super::binding::email_completion_fixture_in(
                Db::open_in_memory().expect("email receiver state"),
                ReceiverJobState::Processing,
            ),
            "  Brain@Example.Test  ",
        ),
        (
            super::binding::email_completion_fixture_in(
                Db::open_in_memory().expect("email receiver state"),
                ReceiverJobState::Processing,
            ),
            "invalid-email-sender",
        ),
    ];

    for (fixture, persisted_sender) in cases {
        fixture
            .db
            .conn
            .execute(
                "UPDATE receiver_jobs SET response_sender = ?2 WHERE job_id = ?1",
                rusqlite::params![fixture.job_id.to_string(), persisted_sender],
            )
            .expect("stage invalid frozen sender");

        fixture
            .db
            .complete_receiver_job_with_binding(&fixture.request())
            .expect("terminalize invalid frozen sender")
            .expect("exact terminal outcome");
        let terminal: (bool, bool) = fixture
            .db
            .conn
            .query_row(
                "SELECT state = 'failed', error_category = 'invalid-request'
                 FROM receiver_deliveries WHERE job_id = ?1",
                [fixture.job_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("load invalid sender outcome");
        assert!(terminal.0, "invalid sender outcome was sendable");
        assert!(
            terminal.1,
            "invalid sender outcome had the wrong content-free category"
        );
        assert!(
            fixture
                .db
                .receiver_job(fixture.job_id)
                .expect("load invalid sender job")
                .is_some_and(|job| job.state() == ReceiverJobState::Failed),
            "invalid sender did not release the agent lane"
        );
        assert!(
            fixture
                .db
                .receiver_answer_cleanup(fixture.job_id)
                .expect("load invalid sender cleanup")
                .is_some(),
            "invalid sender did not persist cleanup authority"
        );
        assert!(
            fixture
                .db
                .claim_next_receiver_delivery("delivery-owner", 2_000, 32_000)
                .expect("inspect invalid sender delivery lane")
                .is_none(),
            "invalid sender entered the provider delivery lane"
        );
    }
}

#[test]
fn exact_completion_conflict_rolls_back_without_changing_the_existing_answer() {
    let fixture = super::binding::completion_fixture(ReceiverJobState::Launched);
    let request = fixture.request();
    fixture
        .db
        .complete_receiver_job_with_binding(&request)
        .expect("record exact answer")
        .expect("exact answer owner");
    let before = fixture
        .db
        .receiver_conversation(fixture.registration.conversation_id())
        .expect("load conversation")
        .expect("durable conversation")
        .transcript_markdown()
        .to_owned();
    let conflicting = ReceiverCompletionRequest {
        answer: "different assistant answer",
        ..request
    };

    let error = fixture
        .db
        .complete_receiver_job_with_binding(&conflicting)
        .expect_err("reject conflicting answer");

    assert_eq!(error.to_string(), "receiver completion conflicts with durable answer");
    assert_eq!(
        fixture
            .db
            .receiver_conversation(fixture.registration.conversation_id())
            .expect("reload conversation")
            .expect("durable conversation")
            .transcript_markdown(),
        before
    );
    assert_eq!(
        fixture
            .db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM receiver_deliveries WHERE job_id = ?1",
                [fixture.job_id.to_string()],
                |row| row.get::<_, i64>(0),
            )
            .expect("count durable answers"),
        1
    );
}

#[test]
fn exact_completion_replay_uses_immutable_evidence_after_later_turn_and_binding_change() {
    let fixture = super::binding::completion_fixture(ReceiverJobState::Processing);
    let request = fixture.request();
    let first = fixture
        .db
        .complete_receiver_job_with_binding(&request)
        .expect("record exact answer")
        .expect("exact answer owner");
    let first_transcript = fixture
        .db
        .receiver_conversation(fixture.registration.conversation_id())
        .expect("load first conversation")
        .expect("durable conversation")
        .transcript_markdown()
        .to_owned();
    let later_transcript = format!("{first_transcript}\n\n## Authenticated user\n\nLater turn");
    let later_binding = ReceiverSessionBinding::new(
        crate::agent::AgentKind::OpenCode,
        "later-native-session",
    )
    .expect("later binding");
    assert!(
        fixture
            .db
            .update_receiver_conversation(
                fixture.registration.conversation_id(),
                &later_transcript,
                Some(&later_binding),
                1_600,
            )
            .expect("advance conversation after first answer")
    );

    let replay = fixture
        .db
        .complete_receiver_job_with_binding(&request)
        .expect("match the immutable first completion")
        .expect("existing exact answer");

    assert!(!replay.newly_recorded());
    assert_eq!(replay.delivery_id(), first.delivery_id());
    let retained = fixture
        .db
        .receiver_conversation(fixture.registration.conversation_id())
        .expect("reload later conversation")
        .expect("durable conversation");
    assert_eq!(retained.transcript_markdown(), later_transcript);
    assert_eq!(retained.binding(), Some(&later_binding));
}

#[test]
fn exact_completion_replay_rejects_a_different_registered_session() {
    let fixture = super::binding::completion_fixture(ReceiverJobState::Processing);
    let request = fixture.request();
    fixture
        .db
        .complete_receiver_job_with_binding(&request)
        .expect("record exact answer")
        .expect("exact answer owner");
    let wrong_registered = crate::agent::AgentSession::new("wrong-registered-session")
        .expect("wrong registered session");
    let crossed = ReceiverSessionAttribution::new(
        fixture.registration.conversation_id(),
        fixture.registration.instance().to_owned(),
        wrong_registered,
        fixture.registration.scope().clone(),
    );
    let conflicting = ReceiverCompletionRequest {
        registration: &crossed,
        ..request
    };

    let error = fixture
        .db
        .complete_receiver_job_with_binding(&conflicting)
        .expect_err("reject a crossed registered session");

    assert_eq!(error.to_string(), "receiver completion conflicts with durable answer");
}

#[test]
fn answer_ready_releases_agent_lane_for_the_next_queued_job() {
    let fixture = super::binding::completion_fixture(ReceiverJobState::Accepted);
    let next = receiver_job(None, 1_600);
    let identity =
        ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let accepted_next = fixture
        .db
        .accept_receiver_job(&next, &identity)
        .expect("accept next job");

    fixture
        .db
        .complete_receiver_job_with_binding(&fixture.request())
        .expect("record exact answer")
        .expect("exact answer owner");
    let next_claim = fixture
        .db
        .claim_next_receiver_run("next-owner", 1_600, 2_600)
        .expect("claim next job")
        .expect("next queued job is independent of delivery");

    assert_eq!(next_claim.job().id(), accepted_next.job_id());
    assert_eq!(next_claim.job().state(), ReceiverJobState::Claimed);
}

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

    assert_eq!(cleanup.job_id(), job_id);
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

#[test]
fn delivery_insert_failure_rolls_back_transcript_binding_and_job_state() {
    for trigger in [
        "CREATE TRIGGER fail_answer_registration
         BEFORE UPDATE OF actual_session_id ON receiver_session_registrations
         WHEN NEW.actual_session_id IS NOT OLD.actual_session_id
         BEGIN SELECT RAISE(FAIL, 'injected registration failure'); END;",
        "CREATE TRIGGER fail_answer_transcript
         BEFORE UPDATE OF transcript_markdown ON receiver_conversations
         WHEN NEW.transcript_markdown != OLD.transcript_markdown
         BEGIN SELECT RAISE(FAIL, 'injected transcript failure'); END;",
        "CREATE TRIGGER fail_answer_insert
         BEFORE INSERT ON receiver_deliveries
         BEGIN SELECT RAISE(FAIL, 'injected answer insert failure'); END;",
        "CREATE TRIGGER fail_answer_cleanup
         BEFORE INSERT ON receiver_answer_cleanups
         BEGIN SELECT RAISE(FAIL, 'injected answer cleanup failure'); END;",
        "CREATE TRIGGER fail_answer_job
         BEFORE UPDATE OF state ON receiver_jobs
         WHEN NEW.state = 'answer-ready'
         BEGIN SELECT RAISE(FAIL, 'injected answer-ready failure'); END;",
    ] {
        let fixture = super::binding::completion_fixture(ReceiverJobState::Processing);
        fixture
            .db
            .conn
            .execute_batch(trigger)
            .expect("install answer failure");

        assert!(
            fixture
                .db
                .complete_receiver_job_with_binding(&fixture.request())
                .is_err()
        );
        assert_completion_rolled_back(&fixture);
    }
}

fn assert_completion_rolled_back(fixture: &super::binding::CompletionFixture) {
    let job = fixture
        .db
        .receiver_job(fixture.job_id)
        .expect("load job")
        .expect("durable job");
    let conversation = fixture
        .db
        .receiver_conversation(fixture.registration.conversation_id())
        .expect("load conversation")
        .expect("durable conversation");
    assert_eq!(job.state(), ReceiverJobState::Processing);
    assert_eq!(
        fixture
            .db
            .conn
            .query_row(
                "SELECT claim_owner FROM receiver_jobs WHERE job_id = ?1",
                [fixture.job_id.to_string()],
                |row| row.get::<_, Option<String>>(0),
            )
            .expect("load rolled-back owner")
            .as_deref(),
        Some("owner")
    );
    assert!(job.completed_at_unix_ms().is_none());
    assert!(conversation.transcript_markdown().is_empty());
    assert!(conversation.binding().is_none());
    assert_eq!(
        fixture
            .db
            .conn
            .query_row("SELECT COUNT(*) FROM receiver_deliveries", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("count rolled-back deliveries"),
        0
    );
    assert_eq!(
        fixture
            .db
            .conn
            .query_row("SELECT COUNT(*) FROM receiver_answer_cleanups", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("count rolled-back answer cleanups"),
        0
    );
}

#[test]
fn blank_and_oversized_answers_fail_before_mutating_state() {
    for answer in [" \n\t".to_owned(), "x".repeat(MAX_RECEIVER_ANSWER_BYTES + 1)] {
        let fixture = super::binding::completion_fixture(ReceiverJobState::Processing);
        let request = ReceiverCompletionRequest {
            answer: &answer,
            ..fixture.request()
        };

        assert!(
            fixture
                .db
                .complete_receiver_job_with_binding(&request)
                .is_err()
        );
        assert_completion_rolled_back(&fixture);
    }
}

#[test]
fn concurrent_identical_completion_records_one_answer_and_one_existing_outcome() {
    let temporary = tempfile::tempdir().expect("temporary receiver state");
    let path = temporary.path().join("state.db");
    let workspace = receiver_workspace_id().to_string();
    let actor = receiver_user_id();
    let first = Db::open_path_with_legacy_identity(&path, &workspace, actor.as_str())
        .expect("open first completion store");
    let fixture = super::binding::completion_fixture_in(first, ReceiverJobState::Processing);
    let second = Db::open_path_with_legacy_identity(&path, &workspace, actor.as_str())
        .expect("open second completion store");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let registration = fixture.registration.clone();
    let completed_session = fixture.completed_session.clone();
    let job_id = fixture.job_id;
    let token = fixture.token;
    let super::binding::CompletionFixture { db: first, .. } = fixture;

    let results = std::thread::scope(|scope| {
        let run = |db: Db, barrier: std::sync::Arc<std::sync::Barrier>| {
            let registration = registration.clone();
            let completed_session = completed_session.clone();
            scope.spawn(move || {
                barrier.wait();
                db.complete_receiver_job_with_binding(&ReceiverCompletionRequest {
                    job_id,
                    token,
                    owner: "owner",
                    registration: &registration,
                    completed_session: &completed_session,
                    answer: "exact assistant answer",
                    observed_at_unix_ms: 1_500,
                    authorized_at_unix_ms: 1_500,
                })
                .expect("serialize exact completion")
                .expect("exact completion outcome")
            })
        };
        let first_handle = run(first, std::sync::Arc::clone(&barrier));
        let second_handle = run(second, barrier);
        [
            first_handle.join().expect("first completion writer"),
            second_handle.join().expect("second completion writer"),
        ]
    });

    assert_eq!(
        results.iter().filter(|outcome| outcome.newly_recorded()).count(),
        1
    );
    assert_eq!(results[0].delivery_id(), results[1].delivery_id());
    let verify = Db::open_path_with_legacy_identity(&path, &workspace, actor.as_str())
        .expect("reopen completion store");
    assert_eq!(
        verify
            .conn
            .query_row("SELECT COUNT(*) FROM receiver_deliveries", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("count concurrent answers"),
        1
    );
}
