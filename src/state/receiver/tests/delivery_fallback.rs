#[test]
fn frozen_fallback_excludes_failed_provider_and_attempted_recipients() {
    let frozen = [
        ReceiverFallbackDestination::sms("+12125550199", "+12125550100")
            .expect("frozen SMS destination"),
        ReceiverFallbackDestination::email("brain@example.test", "already@example.test")
            .expect("frozen attempted email destination"),
        ReceiverFallbackDestination::email("brain@example.test", "safe@example.test")
            .expect("frozen safe email destination"),
    ];
    let plan = plan_receiver_fallback(
        ReceiverProviderCapability::Twilio,
        &["already@example.test"],
        &frozen,
    )
    .expect("one frozen alternate remains safe");

    assert!(
        plan.destination().recipient() == "safe@example.test",
        "fallback selected the wrong frozen destination"
    );
    assert!(plan.notice().chars().count() <= crate::server::reply::SMS_LIMIT);
    assert!(!format!("{plan:?}").contains("safe@example.test"));
}

#[test]
fn fallback_never_uses_later_authority_and_current_single_channel_jobs_stop() {
    assert!(
        plan_receiver_fallback(ReceiverProviderCapability::Twilio, &[], &[]).is_none(),
        "current accepted jobs freeze no alternate authority"
    );
    let later_configuration = [
        ReceiverFallbackDestination::email("brain@example.test", "later@example.test")
            .expect("later email destination"),
    ];
    assert_eq!(later_configuration.len(), 1);
    assert!(
        plan_receiver_fallback(ReceiverProviderCapability::Twilio, &[], &[]).is_none(),
        "later configuration is not an input to the frozen-authority planner"
    );
}

#[test]
fn terminal_single_channel_delivery_persists_no_safe_fallback() {
    let fixture = super::binding::completion_fixture(ReceiverJobState::Processing);
    fixture
        .db
        .complete_receiver_job_with_binding(&fixture.request())
        .expect("record durable answer")
        .expect("exact completion owner");
    let claim = fixture
        .db
        .claim_next_receiver_delivery("terminal-owner", 2_000, 32_000)
        .expect("claim delivery")
        .expect("due delivery");
    assert!(
        fixture
            .db
            .mark_receiver_delivery_io_started(&claim, 2_100)
            .expect("mark provider IO")
    );

    assert_eq!(
        fixture
            .db
            .apply_receiver_delivery_result(
                &claim,
                2_200,
                ReceiverProviderResultClass::PermanentlyRejected(
                    ReceiverDeliveryErrorCategory::ProviderRejected,
                ),
            )
            .expect("apply terminal result"),
        ReceiverDeliveryApplyOutcome::Applied
    );

    let terminal: (String, String, Option<String>, i64) = fixture
        .db
        .conn
        .query_row(
            "SELECT delivery.state, delivery.error_category,
                    delivery.fallback_decision,
                    (SELECT COUNT(*) FROM receiver_deliveries AS fallback
                     WHERE fallback.job_id = delivery.job_id
                       AND fallback.response_kind = 'fallback-notice')
             FROM receiver_deliveries AS delivery WHERE delivery.job_id = ?1",
            [fixture.job_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("load terminal fallback decision");
    assert_eq!(
        terminal,
        (
            "failed".to_owned(),
            "provider-rejected".to_owned(),
            Some("no-safe-fallback".to_owned()),
            0,
        )
    );
}

#[test]
fn terminal_result_atomically_inserts_one_frozen_alternate_notice_without_recursion() {
    let fixture = super::binding::completion_fixture(ReceiverJobState::Processing);
    fixture
        .db
        .complete_receiver_job_with_binding(&fixture.request())
        .expect("record durable answer")
        .expect("exact completion owner");
    let frozen = serde_json::json!([
        {
            "provider": "twilio",
            "sender": "+12125550100",
            "recipient": "+12125550101"
        },
        {
            "provider": "resend",
            "sender": "brain@example.test",
            "recipient": "safe@example.test"
        }
    ])
    .to_string();
    fixture
        .db
        .conn
        .execute(
            "UPDATE receiver_deliveries SET frozen_fallbacks_json = ?2 WHERE job_id = ?1",
            rusqlite::params![fixture.job_id.to_string(), frozen],
        )
        .expect("freeze hypothetical authenticated alternates");
    let claim = fixture
        .db
        .claim_next_receiver_delivery("terminal-owner", 2_000, 32_000)
        .expect("claim primary delivery")
        .expect("primary delivery");
    assert!(
        fixture
            .db
            .mark_receiver_delivery_io_started(&claim, 2_100)
            .expect("mark primary provider IO")
    );

    assert_eq!(
        fixture
            .db
            .apply_receiver_delivery_result(
                &claim,
                2_200,
                ReceiverProviderResultClass::PermanentlyRejected(
                    ReceiverDeliveryErrorCategory::ProviderRejected,
                ),
            )
            .expect("apply primary terminal result"),
        ReceiverDeliveryApplyOutcome::Applied
    );
    assert_eq!(
        fixture
            .db
            .apply_receiver_delivery_result(
                &claim,
                2_201,
                ReceiverProviderResultClass::PermanentlyRejected(
                    ReceiverDeliveryErrorCategory::ProviderRejected,
                ),
            )
            .expect("ignore duplicate primary result"),
        ReceiverDeliveryApplyOutcome::Stale
    );

    let rows: Vec<(String, String, Option<String>, String)> = {
        let mut statement = fixture
            .db
            .conn
            .prepare(
                "SELECT response_kind, state, fallback_decision, envelope_json
                 FROM receiver_deliveries WHERE job_id = ?1
                 ORDER BY created_at_unix_ms, delivery_id",
            )
            .expect("prepare fallback rows");
        statement
            .query_map([fixture.job_id.to_string()], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .expect("query fallback rows")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("collect fallback rows")
    };
    assert_eq!(rows.len(), 2);
    assert_eq!(
        (rows[0].0.as_str(), rows[0].1.as_str(), rows[0].2.as_deref()),
        ("final-answer", "failed", Some("fallback-planned"))
    );
    assert_eq!(
        (rows[1].0.as_str(), rows[1].1.as_str(), rows[1].2.as_deref()),
        ("fallback-notice", "ready", None)
    );
    let fallback_envelope: ReceiverDeliveryEnvelope =
        serde_json::from_str(&rows[1].3).expect("valid frozen fallback envelope");
    let email = fallback_envelope.email().expect("alternate provider is Resend");
    assert!(
        email.recipients() == ["safe@example.test"],
        "fallback selected the wrong frozen recipient"
    );
    assert!(
        email.text()
            == "I couldn’t deliver the full response on the original channel. Please try again there.",
        "fallback notice text changed"
    );
    assert_eq!(
        fixture.db.receiver_job(fixture.job_id).unwrap().unwrap().state(),
        ReceiverJobState::AnswerReady
    );

    let fallback_claim = fixture
        .db
        .claim_next_receiver_delivery("fallback-owner", 2_300, 32_300)
        .expect("claim fallback notice")
        .expect("fallback notice remains durable across ticks");
    assert!(
        fixture
            .db
            .mark_receiver_delivery_io_started(&fallback_claim, 2_400)
            .expect("mark fallback provider IO")
    );
    assert_eq!(
        fixture
            .db
            .apply_receiver_delivery_result(
                &fallback_claim,
                2_500,
                ReceiverProviderResultClass::PermanentlyRejected(
                    ReceiverDeliveryErrorCategory::ProviderRejected,
                ),
            )
            .expect("terminalize fallback notice"),
        ReceiverDeliveryApplyOutcome::Applied
    );
    let final_rows: (i64, Option<String>) = fixture
        .db
        .conn
        .query_row(
            "SELECT COUNT(*), MAX(CASE WHEN response_kind = 'fallback-notice'
                                  THEN fallback_decision END)
             FROM receiver_deliveries WHERE job_id = ?1",
            [fixture.job_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("load non-recursive fallback outcome");
    assert_eq!(final_rows, (2, Some("no-safe-fallback".to_owned())));
}

#[test]
fn terminal_result_rolls_back_when_the_frozen_fallback_cannot_be_inserted() {
    let fixture = super::binding::completion_fixture(ReceiverJobState::Processing);
    fixture
        .db
        .complete_receiver_job_with_binding(&fixture.request())
        .expect("record durable answer")
        .expect("exact completion owner");
    fixture
        .db
        .conn
        .execute(
            "UPDATE receiver_deliveries SET frozen_fallbacks_json = ?2 WHERE job_id = ?1",
            rusqlite::params![
                fixture.job_id.to_string(),
                serde_json::json!([{
                    "provider": "resend",
                    "sender": "brain@example.test",
                    "recipient": "safe@example.test"
                }])
                .to_string(),
            ],
        )
        .expect("freeze authenticated fallback");
    let claim = fixture
        .db
        .claim_next_receiver_delivery("rollback-owner", 2_000, 32_000)
        .expect("claim primary delivery")
        .expect("primary delivery");
    assert!(
        fixture
            .db
            .mark_receiver_delivery_io_started(&claim, 2_100)
            .expect("mark primary provider IO")
    );
    fixture
        .db
        .conn
        .execute_batch(
            "CREATE TRIGGER reject_fallback_insert
             BEFORE INSERT ON receiver_deliveries
             WHEN NEW.response_kind = 'fallback-notice'
             BEGIN
               SELECT RAISE(FAIL, 'injected fallback insert failure');
             END;",
        )
        .expect("install fallback insertion fault");

    let error = fixture
        .db
        .apply_receiver_delivery_result(
            &claim,
            2_200,
            ReceiverProviderResultClass::PermanentlyRejected(
                ReceiverDeliveryErrorCategory::ProviderRejected,
            ),
        )
        .expect_err("fallback insertion failure must roll back the terminal result");

    let retained: (String, Option<String>, Option<String>, i64, String) = fixture
        .db
        .conn
        .query_row(
            "SELECT delivery.state, delivery.error_category, delivery.fallback_decision,
                    (SELECT COUNT(*) FROM receiver_deliveries AS fallback
                     WHERE fallback.job_id = delivery.job_id
                       AND fallback.response_kind = 'fallback-notice'),
                    job.state
             FROM receiver_deliveries AS delivery
             JOIN receiver_jobs AS job ON job.job_id = delivery.job_id
             WHERE delivery.delivery_id = ?1",
            [claim.delivery_id().to_string()],
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
        .expect("load rolled-back terminal result");
    assert!(error.to_string().contains("injected fallback insert failure"));
    assert_eq!(
        retained,
        (
            "delivering".to_owned(),
            None,
            None,
            0,
            "delivering".to_owned(),
        )
    );
}

#[test]
fn concurrent_terminal_results_create_one_restart_durable_fallback_notice() {
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
    fixture
        .db
        .conn
        .execute(
            "UPDATE receiver_deliveries SET frozen_fallbacks_json = ?2 WHERE job_id = ?1",
            rusqlite::params![
                fixture.job_id.to_string(),
                serde_json::json!([{
                    "provider": "resend",
                    "sender": "brain@example.test",
                    "recipient": "safe@example.test"
                }])
                .to_string(),
            ],
        )
        .expect("freeze authenticated fallback");
    let claim = fixture
        .db
        .claim_next_receiver_delivery("terminal-race-owner", 2_000, 32_000)
        .expect("claim primary delivery")
        .expect("primary delivery");
    assert!(
        fixture
            .db
            .mark_receiver_delivery_io_started(&claim, 2_100)
            .expect("mark primary provider IO")
    );
    let job_id = fixture.job_id;
    drop(fixture);

    let start = std::sync::Arc::new(std::sync::Barrier::new(2));
    let mut handles = Vec::new();
    for _ in 0..2 {
        let path = path.clone();
        let workspace = workspace.clone();
        let actor = actor.clone();
        let claim = claim.clone();
        let start = start.clone();
        handles.push(std::thread::spawn(move || {
            let db = Db::open_path_with_legacy_identity(&path, &workspace, actor.as_str())
                .expect("racing receiver state");
            start.wait();
            db.apply_receiver_delivery_result(
                &claim,
                2_200,
                ReceiverProviderResultClass::PermanentlyRejected(
                    ReceiverDeliveryErrorCategory::ProviderRejected,
                ),
            )
            .expect("apply racing terminal result")
        }));
    }
    let outcomes = handles
        .into_iter()
        .map(|handle| handle.join().expect("join terminal result racer"))
        .collect::<Vec<_>>();
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == ReceiverDeliveryApplyOutcome::Applied)
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == ReceiverDeliveryApplyOutcome::Stale)
            .count(),
        1
    );

    let restarted = Db::open_path_with_legacy_identity(&path, &workspace, actor.as_str())
        .expect("restart receiver state");
    let durable: (i64, String) = restarted
        .conn
        .query_row(
            "SELECT COUNT(*), job.state
             FROM receiver_deliveries AS delivery
             JOIN receiver_jobs AS job ON job.job_id = delivery.job_id
             WHERE delivery.job_id = ?1 AND delivery.response_kind = 'fallback-notice'",
            [job_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("load restart-durable fallback");
    assert_eq!(durable, (1, "answer-ready".to_owned()));
    let fallback = restarted
        .claim_next_receiver_delivery("restart-fallback-owner", 2_300, 32_300)
        .expect("claim fallback after restart")
        .expect("restart retained fallback authority");
    assert_eq!(fallback.job_id(), job_id);
    assert_eq!(fallback.provider(), ReceiverProviderCapability::Resend);
}

#[test]
fn replay_window_terminalization_uses_only_the_frozen_alternate() {
    let fixture = super::binding::completion_fixture(ReceiverJobState::Processing);
    fixture
        .db
        .complete_receiver_job_with_binding(&fixture.request())
        .expect("record durable answer")
        .expect("exact completion owner");
    let primary = serde_json::json!({
        "channel": "email",
        "value": {
            "sender": "brain@example.test",
            "recipients": ["member@example.test"],
            "subject": "Re: Private subject",
            "text": "private answer",
            "html": "<p>private answer</p>",
            "in_reply_to": null,
            "references": null,
            "provider_email_id": null
        }
    })
    .to_string();
    let frozen = serde_json::json!([{
        "provider": "twilio",
        "sender": "+12125550100",
        "recipient": "+12125550101"
    }])
    .to_string();
    fixture
        .db
        .conn
        .execute(
            "UPDATE receiver_deliveries
             SET envelope_json = ?2, frozen_fallbacks_json = ?3,
                 state = 'retrying', attempt_count = 1,
                 first_attempt_at_unix_ms = 100, retry_at_unix_ms = 200
             WHERE job_id = ?1",
            rusqlite::params![fixture.job_id.to_string(), primary, frozen],
        )
        .expect("stage expired replay with frozen alternate");
    fixture
        .db
        .conn
        .execute(
            "UPDATE receiver_jobs SET state = 'retrying' WHERE job_id = ?1",
            [fixture.job_id.to_string()],
        )
        .expect("stage delivery-lane retry");

    let records = crate::logging::capture_receiver_lifecycle(|| {
        assert_eq!(
            fixture
                .db
                .reconcile_expired_receiver_deliveries(86_400_101)
                .expect("reconcile expired replay"),
            1
        );
    });
    assert_receiver_lifecycle_records(
        &records,
        &[
            "receiver lifecycle event=delivery-result delivery_phase=ambiguous reason=idempotency-window-expired",
            "receiver lifecycle event=terminal-advancement phase=answer-ready queue_depth=0 reason=idempotency-window-expired",
        ],
    );

    let rows: Vec<(String, String, Option<String>)> = {
        let mut statement = fixture
            .db
            .conn
            .prepare(
                "SELECT response_kind, state, fallback_decision
                 FROM receiver_deliveries WHERE job_id = ?1
                 ORDER BY created_at_unix_ms, delivery_id",
            )
            .expect("prepare replay fallback rows");
        statement
            .query_map([fixture.job_id.to_string()], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .expect("query replay fallback rows")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("collect replay fallback rows")
    };
    assert_eq!(
        rows,
        vec![
            (
                "final-answer".to_owned(),
                "ambiguous".to_owned(),
                Some("fallback-planned".to_owned())
            ),
            ("fallback-notice".to_owned(), "ready".to_owned(), None),
        ]
    );
    assert_eq!(
        fixture.db.receiver_job(fixture.job_id).unwrap().unwrap().state(),
        ReceiverJobState::AnswerReady
    );
}

#[test]
fn claim_logs_expired_retry_terminalization_after_its_commit() {
    let fixture = super::binding::completion_fixture(ReceiverJobState::Processing);
    fixture
        .db
        .complete_receiver_job_with_binding(&fixture.request())
        .expect("record durable answer")
        .expect("exact completion owner");
    fixture
        .db
        .conn
        .execute(
            "UPDATE receiver_deliveries
             SET state = 'retrying', attempt_count = 1,
                 first_attempt_at_unix_ms = 100, retry_at_unix_ms = 200,
                 frozen_fallbacks_json = '[]', envelope_json = ?2
             WHERE job_id = ?1",
            rusqlite::params![
                fixture.job_id.to_string(),
                serde_json::json!({
                    "channel": "email",
                    "value": {
                        "sender": "brain@example.test",
                        "recipients": ["member@example.test"],
                        "subject": "Re: Private subject",
                        "text": "private answer",
                        "html": "<p>private answer</p>",
                        "in_reply_to": null,
                        "references": null,
                        "provider_email_id": null
                    }
                })
                .to_string(),
            ],
        )
        .expect("stage expired retry without fallback");
    fixture
        .db
        .conn
        .execute(
            "UPDATE receiver_jobs SET state = 'retrying' WHERE job_id = ?1",
            [fixture.job_id.to_string()],
        )
        .expect("stage retrying delivery job");

    let records = crate::logging::capture_receiver_lifecycle(|| {
        assert!(
            fixture
                .db
                .claim_next_receiver_delivery("later-owner", 86_400_101, 86_430_101)
                .expect("terminalize expired retry during claim")
                .is_none(),
            "expired retry remained claimable"
        );
    });

    assert_receiver_lifecycle_records(
        &records,
        &[
            "receiver lifecycle event=delivery-result delivery_phase=ambiguous reason=idempotency-window-expired",
            "receiver lifecycle event=terminal-advancement phase=failed queue_depth=0 reason=idempotency-window-expired",
        ],
    );
}

#[test]
fn final_pre_io_expiry_uses_only_the_frozen_alternate() {
    let fixture = super::binding::completion_fixture(ReceiverJobState::Processing);
    fixture
        .db
        .complete_receiver_job_with_binding(&fixture.request())
        .expect("record durable answer")
        .expect("exact completion owner");
    fixture
        .db
        .conn
        .execute(
            "UPDATE receiver_deliveries
             SET envelope_json = ?2, frozen_fallbacks_json = ?3,
                 state = 'retrying', attempt_count = 1,
                 first_attempt_at_unix_ms = 100, retry_at_unix_ms = 200
             WHERE job_id = ?1",
            rusqlite::params![
                fixture.job_id.to_string(),
                serde_json::json!({
                    "channel": "email",
                    "value": {
                        "sender": "brain@example.test",
                        "recipients": ["member@example.test"],
                        "subject": "Re: Private subject",
                        "text": "private answer",
                        "html": "<p>private answer</p>",
                        "in_reply_to": null,
                        "references": null,
                        "provider_email_id": null
                    }
                })
                .to_string(),
                serde_json::json!([{
                    "provider": "twilio",
                    "sender": "+12125550100",
                    "recipient": "+12125550101"
                }])
                .to_string(),
            ],
        )
        .expect("stage exact-window retry");
    fixture
        .db
        .conn
        .execute(
            "UPDATE receiver_jobs SET state = 'retrying' WHERE job_id = ?1",
            [fixture.job_id.to_string()],
        )
        .expect("stage delivery-lane retry");
    let claim = fixture
        .db
        .claim_next_receiver_delivery("window-owner", 86_400_100, 86_430_100)
        .expect("claim at exact replay deadline")
        .expect("exact replay deadline remains eligible");

    let records = crate::logging::capture_receiver_lifecycle(|| {
        assert!(
            !fixture
                .db
                .mark_receiver_delivery_io_started(&claim, 86_400_101)
                .expect("terminalize after exact replay deadline")
        );
    });

    assert_receiver_lifecycle_records(
        &records,
        &[
            "receiver lifecycle event=delivery-result delivery_phase=ambiguous reason=idempotency-window-expired",
            "receiver lifecycle event=terminal-advancement phase=answer-ready queue_depth=0 reason=idempotency-window-expired",
        ],
    );

    let outcome: (String, Option<String>, i64, String) = fixture
        .db
        .conn
        .query_row(
            "SELECT source.state, source.fallback_decision,
                    (SELECT COUNT(*) FROM receiver_deliveries AS fallback
                     WHERE fallback.job_id = source.job_id
                       AND fallback.response_kind = 'fallback-notice'),
                    job.state
             FROM receiver_deliveries AS source
             JOIN receiver_jobs AS job ON job.job_id = source.job_id
             WHERE source.delivery_id = ?1",
            [claim.delivery_id().to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("load pre-IO fallback outcome");
    assert_eq!(
        outcome,
        (
            "ambiguous".to_owned(),
            Some("fallback-planned".to_owned()),
            1,
            "answer-ready".to_owned(),
        )
    );
}
