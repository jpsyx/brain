#[test]
fn durable_agent_transitions_emit_stable_records_only_after_success() {
    let db = Db::open_in_memory().expect("receiver state");
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let job = receiver_job(Some("private-provider-id"), 100);

    let records = crate::logging::capture_receiver_lifecycle(|| {
        let accepted = db
            .accept_receiver_job(&job, &identity)
            .expect("accept receiver job");
        register_observation_session(
            &db,
            accepted.conversation_id(),
            &job,
            "private-instance",
            "private-session",
        );
        let run = db
            .claim_next_receiver_run("private-owner", 1_000, 2_000)
            .expect("claim receiver job")
            .expect("receiver claim");
        assert!(db
            .prepare_receiver_job_launch(run.job().id(), "private-owner", 1_100)
            .expect("prepare launch"));
        assert!(db
            .commit_receiver_job_launch(
                run.job().id(),
                "private-owner",
                &launch_observation(
                    run.job().token(),
                    "private-instance",
                    "private-session",
                    1_200,
                ),
            )
            .expect("commit launch"));
        assert!(db
            .apply_receiver_observation(
                run.job().id(),
                "private-owner",
                &observation(
                    run.job().token(),
                    "private-instance",
                    "private-session",
                    ReceiverNonterminalObservationPhase::Accepted,
                    1,
                    1_300,
                ),
            )
            .expect("record acceptance"));
        assert!(db
            .apply_receiver_observation(
                run.job().id(),
                "private-owner",
                &observation(
                    run.job().token(),
                    "private-instance",
                    "private-session",
                    ReceiverNonterminalObservationPhase::Progressing,
                    2,
                    1_400,
                ),
            )
            .expect("record progress"));
    });

    assert_receiver_lifecycle_records(
        &records,
        &[
            "receiver lifecycle event=ingress phase=queued queue_depth=1",
            "receiver lifecycle event=claim phase=claimed queue_depth=1",
            "receiver lifecycle event=launch phase=launched recovery=not-active",
            "receiver lifecycle event=acceptance phase=accepted",
            "receiver lifecycle event=progress phase=processing",
        ],
    );
    let rendered = records.join("\n");
    for private in [
        "private-provider-id",
        "private-instance",
        "private-session",
        "private-owner",
        receiver_workspace_id().to_string().as_str(),
    ] {
        assert!(!rendered.contains(private), "lifecycle log leaked {private}");
    }
}

#[test]
fn rejected_transition_emits_no_lifecycle_record() {
    let db = Db::open_in_memory().expect("receiver state");
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let accepted = db
        .accept_receiver_job(&receiver_job(None, 100), &identity)
        .expect("accept job");

    let records = crate::logging::capture_receiver_lifecycle(|| {
        assert!(!db
            .prepare_receiver_job_launch(accepted.job_id(), "stale-owner", 1_000)
            .expect("reject unclaimed launch"));
    });

    assert!(records.is_empty(), "rejected transition was logged");
}

#[test]
fn committed_ingress_logs_when_summary_enrichment_is_unavailable() {
    let db = Db::open_in_memory().expect("receiver state");
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let malformed = db
        .accept_receiver_job(&receiver_job(Some("malformed-prior"), 100), &identity)
        .expect("accept prior job");
    db.conn
        .pragma_update(None, "ignore_check_constraints", true)
        .expect("allow malformed summary fixture");
    db.conn
        .execute(
            "UPDATE receiver_jobs SET state = 'mystery' WHERE job_id = ?1",
            [malformed.job_id().to_string()],
        )
        .expect("stage malformed finite state");
    db.conn
        .pragma_update(None, "ignore_check_constraints", false)
        .expect("restore receiver constraints");

    let records = crate::logging::capture_receiver_lifecycle(|| {
        db.accept_receiver_job(&receiver_job(Some("committed-next"), 200), &identity)
            .expect("commit next ingress");
    });

    assert_receiver_lifecycle_records(
        &records,
        &["receiver lifecycle event=ingress phase=queued queue_depth=unavailable"],
    );
}

#[test]
fn answer_and_acknowledged_delivery_log_only_finite_post_commit_state() {
    let fixture = super::binding::completion_fixture(ReceiverJobState::Processing);

    let answer_records = crate::logging::capture_receiver_lifecycle(|| {
        fixture
            .db
            .complete_receiver_job_with_binding(&fixture.request())
            .expect("complete answer")
            .expect("completion owner");
    });
    assert_receiver_lifecycle_records(
        &answer_records,
        &["receiver lifecycle event=answer-readiness phase=answer-ready cleanup_gated=0"],
    );

    let claim = fixture
        .db
        .claim_next_receiver_delivery("private-delivery-owner", 2_000, 32_000)
        .expect("claim delivery")
        .expect("ready delivery");
    assert!(fixture
        .db
        .mark_receiver_delivery_io_started(&claim, 2_100)
        .expect("mark provider IO"));
    let reference = ReceiverProviderReference::parse("SM0123456789abcdef0123456789abcdef")
        .expect("provider reference");

    let delivery_records = crate::logging::capture_receiver_lifecycle(|| {
        assert_eq!(
            fixture
                .db
                .apply_receiver_delivery_result(
                    &claim,
                    2_200,
                    ReceiverProviderResultClass::Acknowledged(reference),
                )
                .expect("apply acknowledged result"),
            ReceiverDeliveryApplyOutcome::Applied,
        );
    });

    assert_receiver_lifecycle_records(
        &delivery_records,
        &[
            "receiver lifecycle event=delivery-result delivery_phase=acknowledged reason=provider-acknowledged",
            "receiver lifecycle event=terminal-advancement phase=done queue_depth=0 reason=provider-acknowledged",
        ],
    );
}
