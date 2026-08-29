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
