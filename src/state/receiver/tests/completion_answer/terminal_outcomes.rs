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
            "Brain@Example.Test",
        ),
        (
            super::binding::email_completion_fixture_in(
                Db::open_in_memory().expect("email receiver state"),
                ReceiverJobState::Processing,
            ),
            "Brain <brain@example.test>",
        ),
        (
            super::binding::email_completion_fixture_in(
                Db::open_in_memory().expect("email receiver state"),
                ReceiverJobState::Processing,
            ),
            "brain@example.test>",
        ),
        (
            super::binding::email_completion_fixture_in(
                Db::open_in_memory().expect("email receiver state"),
                ReceiverJobState::Processing,
            ),
            "brain..reply@example.test",
        ),
        (
            super::binding::email_completion_fixture_in(
                Db::open_in_memory().expect("email receiver state"),
                ReceiverJobState::Processing,
            ),
            "brain@-example.test",
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
