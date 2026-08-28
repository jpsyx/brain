fn answer_ready_fixture() -> super::binding::CompletionFixture {
    let fixture = super::binding::completion_fixture(ReceiverJobState::Processing);
    fixture
        .db
        .complete_receiver_job_with_binding(&fixture.request())
        .expect("record durable answer")
        .expect("exact completion owner");
    fixture
}

#[test]
fn oldest_due_final_answer_claim_is_independent_and_exact() {
    let first = answer_ready_fixture();
    let later_job = ReceiverJobId::from(uuid::Uuid::new_v4());
    let later_token = ReceiverJobToken::new();
    let later_delivery = ReceiverDeliveryId::new();
    first
        .db
        .conn
        .execute(
            "INSERT INTO receiver_jobs
               (job_id, job_token, workspace_id, conversation_id, channel, inbound_json,
                state, received_at_unix_ms, updated_at_unix_ms)
             SELECT ?1, ?2, workspace_id, conversation_id, channel, inbound_json,
                    'answer-ready', 200, 200
             FROM receiver_jobs WHERE job_id = ?3",
            rusqlite::params![
                later_job.to_string(),
                later_token.to_string(),
                first.job_id.to_string()
            ],
        )
        .expect("seed later answer-ready job");
    first
        .db
        .conn
        .execute(
            "INSERT INTO receiver_deliveries
               (delivery_id, job_id, job_token, response_kind, envelope_json, state,
                attempt_count, created_at_unix_ms, updated_at_unix_ms)
             SELECT ?1, ?2, ?3, 'final-answer', envelope_json, 'ready', 0, 200, 200
             FROM receiver_deliveries WHERE job_id = ?4",
            rusqlite::params![
                later_delivery.to_string(),
                later_job.to_string(),
                later_token.to_string(),
                first.job_id.to_string()
            ],
        )
        .expect("seed later delivery");
    first
        .db
        .conn
        .execute(
            "UPDATE receiver_deliveries SET created_at_unix_ms = 100 WHERE job_id = ?1",
            [first.job_id.to_string()],
        )
        .expect("order first delivery");

    let claim = first
        .db
        .claim_next_receiver_delivery("delivery-owner", 2_000, 32_000)
        .expect("claim delivery")
        .expect("due delivery");

    assert_eq!(claim.job_id(), first.job_id);
    assert_eq!(claim.token(), first.token);
    assert_eq!(claim.owner(), "delivery-owner");
    assert_eq!(claim.expires_at_unix_ms(), 32_000);
    assert_eq!(claim.attempt_count(), 1);
    assert_eq!(claim.provider(), ReceiverProviderCapability::Twilio);
    assert!(claim.envelope().sms().is_some());
    assert_ne!(claim.delivery_id().to_string(), claim.attempt_id().to_string());
    assert_eq!(
        first
            .db
            .receiver_job(first.job_id)
            .expect("load claimed job")
            .expect("claimed job")
            .state(),
        ReceiverJobState::Delivering
    );
    let later_claim = first
        .db
        .claim_next_receiver_delivery("racing-owner", 2_000, 32_000)
        .expect("claim independent later delivery")
        .expect("later delivery remains claimable");
    assert_eq!(later_claim.job_id(), later_job);
    assert_ne!(later_claim.attempt_id(), claim.attempt_id());
    assert!(
        first
            .db
            .claim_next_receiver_delivery("third-owner", 2_000, 32_000)
            .expect("third claim")
            .is_none(),
        "each exact delivery can be claimed only once"
    );

    assert_eq!(
        first
            .db
            .receiver_job(later_job)
            .expect("load independent job")
            .expect("independent job")
            .state(),
        ReceiverJobState::Delivering,
        "the independent delivery claim owns the later job"
    );
    assert!(
        first
            .db
            .claim_next_receiver_run("agent-owner", 2_000, 32_000)
            .expect("ordinary agent claim remains independent")
            .is_none(),
        "delivery jobs never return to the ordinary agent lane"
    );
}

#[test]
fn exact_result_cas_retries_without_returning_to_agent_processing() {
    let fixture = answer_ready_fixture();
    let claim = fixture
        .db
        .claim_next_receiver_delivery("delivery-owner", 2_000, 32_000)
        .expect("claim delivery")
        .expect("due delivery");
    assert!(fixture
        .db
        .mark_receiver_delivery_io_started(&claim, 2_100)
        .expect("mark provider IO"));

    let applied = fixture
        .db
        .apply_receiver_delivery_result(
            &claim,
            2_200,
            ReceiverProviderResultClass::DefinitelyNotAccepted(
                ReceiverDeliveryErrorCategory::TransportUnavailable,
            ),
        )
        .expect("apply retry result");

    assert_eq!(applied, ReceiverDeliveryApplyOutcome::Applied);
    assert_eq!(
        fixture
            .db
            .receiver_job(fixture.job_id)
            .expect("load retrying job")
            .expect("retrying job")
            .state(),
        ReceiverJobState::Retrying
    );
    let row: (String, i64, i64, Option<String>) = fixture
        .db
        .conn
        .query_row(
            "SELECT state, attempt_count, retry_at_unix_ms, claim_owner
             FROM receiver_deliveries WHERE job_id = ?1",
            [fixture.job_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("load retry row");
    assert_eq!(row, ("retrying".to_owned(), 1, 62_200, None));
}

#[test]
fn stale_and_duplicate_provider_results_are_idempotently_ignored() {
    let fixture = answer_ready_fixture();
    let claim = fixture
        .db
        .claim_next_receiver_delivery("delivery-owner", 2_000, 32_000)
        .expect("claim delivery")
        .expect("due delivery");
    assert!(fixture
        .db
        .mark_receiver_delivery_io_started(&claim, 2_100)
        .expect("mark provider IO"));
    let reference = ReceiverProviderReference::parse("SM0123456789abcdef0123456789abcdef")
        .expect("provider reference");

    assert_eq!(
        fixture
            .db
            .apply_receiver_delivery_result(
                &claim,
                2_200,
                ReceiverProviderResultClass::Acknowledged(reference.clone()),
            )
            .expect("acknowledge delivery"),
        ReceiverDeliveryApplyOutcome::Applied
    );
    assert_eq!(
        fixture
            .db
            .apply_receiver_delivery_result(
                &claim,
                2_300,
                ReceiverProviderResultClass::Acknowledged(reference),
            )
            .expect("ignore duplicate result"),
        ReceiverDeliveryApplyOutcome::Stale
    );
    assert_eq!(
        fixture
            .db
            .receiver_job(fixture.job_id)
            .expect("load done job")
            .expect("done job")
            .state(),
        ReceiverJobState::Done
    );
}

#[test]
fn restart_reconciliation_distinguishes_pre_spawn_and_twilio_io() {
    let safe = answer_ready_fixture();
    let safe_claim = safe
        .db
        .claim_next_receiver_delivery("departed-owner", 2_000, 3_000)
        .expect("claim safe delivery")
        .expect("safe delivery");

    assert_eq!(
        safe.db
            .reconcile_expired_receiver_deliveries(3_000)
            .expect("reconcile pre-spawn claim"),
        1
    );
    assert_eq!(
        safe.db
            .receiver_job(safe_claim.job_id())
            .expect("load safe job")
            .expect("safe job")
            .state(),
        ReceiverJobState::AnswerReady
    );

    let ambiguous = answer_ready_fixture();
    let ambiguous_claim = ambiguous
        .db
        .claim_next_receiver_delivery("departed-owner", 4_000, 5_000)
        .expect("claim ambiguous delivery")
        .expect("ambiguous delivery");
    assert!(ambiguous
        .db
        .mark_receiver_delivery_io_started(&ambiguous_claim, 4_100)
        .expect("mark provider IO"));

    assert_eq!(
        ambiguous
            .db
            .reconcile_expired_receiver_deliveries(5_000)
            .expect("reconcile provider IO"),
        1
    );
    let row: (String, String) = ambiguous
        .db
        .conn
        .query_row(
            "SELECT state, ambiguity_reason FROM receiver_deliveries WHERE job_id = ?1",
            [ambiguous_claim.job_id().to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("load ambiguous delivery");
    assert_eq!(
        row,
        (
            "ambiguous".to_owned(),
            "provider-acceptance-unknown".to_owned()
        )
    );
    assert_eq!(
        ambiguous
            .db
            .receiver_job(ambiguous_claim.job_id())
            .expect("load ambiguous job")
            .expect("ambiguous job")
            .state(),
        ReceiverJobState::Failed
    );
}

#[test]
fn saturated_pre_spawn_claim_is_released_without_recording_an_attempt() {
    let fixture = answer_ready_fixture();
    let claim = fixture
        .db
        .claim_next_receiver_delivery("delivery-owner", 2_000, 32_000)
        .expect("claim delivery")
        .expect("due delivery");

    assert!(fixture
        .db
        .release_receiver_delivery_before_io(&claim, 2_100)
        .expect("release pre-spawn delivery"));
    let replay = fixture
        .db
        .claim_next_receiver_delivery("next-owner", 2_100, 32_100)
        .expect("reclaim delivery")
        .expect("released delivery is immediately due");

    assert_eq!(replay.delivery_id(), claim.delivery_id());
    assert_ne!(replay.attempt_id(), claim.attempt_id());
    assert_eq!(replay.attempt_count(), 1);
}

#[test]
fn worker_publication_failure_after_io_marker_restores_an_unsent_twilio_attempt() {
    let fixture = answer_ready_fixture();
    let claim = fixture
        .db
        .claim_next_receiver_delivery("delivery-owner", 2_000, 32_000)
        .expect("claim delivery")
        .expect("due delivery");
    assert_eq!(claim.provider(), ReceiverProviderCapability::Twilio);
    assert!(fixture
        .db
        .mark_receiver_delivery_io_started(&claim, 2_100)
        .expect("mark publication boundary"));

    assert!(fixture
        .db
        .release_receiver_delivery_after_failed_publication(&claim, 2_100)
        .expect("release work proven not to have reached the worker"));

    let row: (String, i64, Option<i64>, i64) = fixture
        .db
        .conn
        .query_row(
            "SELECT state, attempt_count, first_attempt_at_unix_ms, provider_io_started
             FROM receiver_deliveries WHERE job_id = ?1",
            [fixture.job_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("load safely released publication");
    assert_eq!(row, ("ready".to_owned(), 0, None, 0));
    assert_eq!(
        fixture
            .db
            .receiver_job(fixture.job_id)
            .expect("load safely released job")
            .expect("job remains")
            .state(),
        ReceiverJobState::AnswerReady
    );
}

#[test]
fn concurrent_delivery_claim_race_has_one_exact_winner() {
    let temporary = tempfile::tempdir().expect("temporary state directory");
    let path = temporary.path().join("state.db");
    let workspace = super::support::receiver_workspace_id().to_string();
    let actor = super::support::receiver_user_id();
    let fixture = super::binding::completion_fixture_in(
        Db::open_path_with_legacy_identity(&path, &workspace, actor.as_str())
            .expect("file-backed receiver state"),
        ReceiverJobState::Processing,
    );
    fixture
        .db
        .complete_receiver_job_with_binding(&fixture.request())
        .expect("record durable answer")
        .expect("exact completion owner");
    let expected_job = fixture.job_id;
    let expected_token = fixture.token;
    drop(fixture);
    let start = std::sync::Arc::new(std::sync::Barrier::new(2));
    #[allow(clippy::needless_collect)]
    let handles = (0..2)
        .map(|index| {
            let path = path.clone();
            let start = start.clone();
            let workspace = workspace.clone();
            let actor = actor.clone();
            std::thread::spawn(move || {
                let db = Db::open_path_with_legacy_identity(&path, &workspace, actor.as_str())
                    .expect("racing state connection");
                start.wait();
                db.claim_next_receiver_delivery(
                    &format!("delivery-racer-{index}"),
                    2_000,
                    32_000,
                )
                .expect("race delivery claim")
            })
        })
        .collect::<Vec<_>>();
    let winners = handles
        .into_iter()
        .filter_map(|handle| handle.join().expect("join delivery racer"))
        .collect::<Vec<_>>();

    assert_eq!(winners.len(), 1);
    assert_eq!(winners[0].job_id(), expected_job);
    assert_eq!(winners[0].token(), expected_token);
}

#[test]
fn resend_io_restart_replays_frozen_answer_and_envelope_inside_the_window() {
    let fixture = answer_ready_fixture();
    let email_envelope = serde_json::json!({
        "channel": "email",
        "value": {
            "sender": "brain@example.test",
            "recipients": ["member@example.test"],
            "subject": "Re: Frozen subject",
            "text": "frozen private answer",
            "html": "<p>frozen private answer</p>",
            "in_reply_to": "<message@example.test>",
            "references": "<message@example.test>",
            "provider_email_id": "provider-email-id"
        }
    })
    .to_string();
    fixture
        .db
        .conn
        .execute(
            "UPDATE receiver_deliveries SET envelope_json = ?2 WHERE job_id = ?1",
            rusqlite::params![fixture.job_id.to_string(), email_envelope],
        )
        .expect("replace fixture with frozen email envelope");
    let before: (String, String, String) = fixture
        .db
        .conn
        .query_row(
            "SELECT delivery.completion_evidence_json, delivery.envelope_json,
                    conversation.transcript_markdown
             FROM receiver_deliveries AS delivery
             JOIN receiver_jobs AS job ON job.job_id = delivery.job_id
             JOIN receiver_conversations AS conversation
               ON conversation.conversation_id = job.conversation_id
             WHERE delivery.job_id = ?1",
            [fixture.job_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("load immutable delivery content");
    let claim = fixture
        .db
        .claim_next_receiver_delivery("departed-owner", 4_000, 5_000)
        .expect("claim resend delivery")
        .expect("resend delivery");
    assert!(fixture
        .db
        .mark_receiver_delivery_io_started(&claim, 4_100)
        .expect("mark provider IO"));

    assert_eq!(
        fixture
            .db
            .reconcile_expired_receiver_deliveries(5_000)
            .expect("reconcile resend provider IO"),
        1
    );
    let after: (String, String, String, String, i64) = fixture
        .db
        .conn
        .query_row(
            "SELECT delivery.completion_evidence_json, delivery.envelope_json,
                    conversation.transcript_markdown, delivery.state,
                    delivery.retry_at_unix_ms
             FROM receiver_deliveries AS delivery
             JOIN receiver_jobs AS job ON job.job_id = delivery.job_id
             JOIN receiver_conversations AS conversation
               ON conversation.conversation_id = job.conversation_id
             WHERE delivery.job_id = ?1",
            [fixture.job_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .expect("load reconciled resend delivery");

    assert_eq!((&after.0, &after.1, &after.2), (&before.0, &before.1, &before.2));
    assert_eq!(after.3, "retrying");
    assert_eq!(after.4, 65_000);
}
