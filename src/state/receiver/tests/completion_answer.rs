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
