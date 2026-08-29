#[test]
fn missing_exact_native_session_terminalizes_without_an_unacknowledgeable_cleanup() {
    let fixture = accepted_run("missing-native-recovery");
    fixture
        .db
        .conn
        .execute("DELETE FROM receiver_session_registrations", [])
        .expect("remove exact session registration");
    let effect = fixture
        .db
        .reconcile_next_receiver_job(301_400)
        .expect("reconcile missing native session")
        .expect("terminal effect");
    assert_eq!(effect.action(), ReceiverReconciliationAction::TerminalFailure);
    assert_eq!(
        effect.reason(),
        ReceiverReconciliationReason::NativeSessionUnavailable
    );
    assert_eq!(effect.cleanup_instance(), None);
    assert_eq!(effect.cleanup_session_id(), None);
    let terminal = fixture
        .db
        .receiver_job(fixture.job_id)
        .expect("load terminal job")
        .expect("terminal job");
    assert_eq!(terminal.state(), ReceiverJobState::AnswerReady);
    assert_eq!(terminal.recovery_count(), 0);
    assert_eq!(terminal.recovery_cleanup_instance(), None);
    assert_eq!(terminal.recovery_cleanup_session_id(), None);
    assert_eq!(
        terminal.last_error(),
        Some(ReceiverReconciliationReason::NativeSessionUnavailable.as_str())
    );
    let delivery: (String, String) = fixture
        .db
        .conn
        .query_row(
            "SELECT response_kind, state FROM receiver_deliveries WHERE job_id = ?1",
            [fixture.job_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("load terminal unavailable response");
    assert_eq!(delivery, ("unavailable-notice".to_owned(), "ready".to_owned()));
}

#[test]
fn exhausted_preacceptance_budget_terminalizes_and_releases_fifo() {
    let fixture = launched_run("exhausted-preacceptance", 400_000);
    fixture
        .db
        .conn
        .execute(
            "UPDATE receiver_jobs SET retry_count = 2 WHERE job_id = ?1",
            [fixture.job_id.to_string()],
        )
        .expect("put job on its final launch attempt");
    let later = receiver_job(Some("later-after-terminal"), 200);
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let later_acceptance = fixture
        .db
        .accept_receiver_job(&later, &identity)
        .expect("accept later receiver job");
    let effect = fixture
        .db
        .reconcile_next_receiver_job(91_200)
        .expect("reconcile exhausted launch")
        .expect("terminal effect");
    assert_eq!(effect.action(), ReceiverReconciliationAction::TerminalFailure);
    assert_eq!(
        effect.reason(),
        ReceiverReconciliationReason::PreAcceptanceExhausted
    );
    let terminal = fixture
        .db
        .receiver_job(fixture.job_id)
        .expect("load terminal job")
        .expect("terminal job");
    assert_eq!(terminal.state(), ReceiverJobState::Failed);
    assert_eq!(terminal.retry_count(), 3);
    assert_eq!(terminal.recovery_count(), 0);
    assert_eq!(
        fixture
            .db
            .claim_next_receiver_run("next-owner", 91_200, 121_200)
            .expect("claim after terminal")
            .expect("later work is unblocked")
            .job()
            .id(),
        later_acceptance.job_id()
    );
}

#[test]
fn absolute_expiry_terminalizes_even_when_progress_deadline_is_later() {
    let fixture = accepted_run("absolute-expiry");
    fixture
        .db
        .conn
        .execute(
            "UPDATE receiver_jobs SET progress_expires_at_unix_ms = 2000000
             WHERE job_id = ?1",
            [fixture.job_id.to_string()],
        )
        .expect("simulate a renewed progress lease at the absolute boundary");
    let effect = fixture
        .db
        .reconcile_next_receiver_job(1_801_300)
        .expect("reconcile absolute expiry")
        .expect("terminal effect");
    assert_eq!(effect.action(), ReceiverReconciliationAction::TerminalFailure);
    assert_eq!(
        effect.reason(),
        ReceiverReconciliationReason::AbsoluteWorkExpired
    );
}

#[test]
fn incomplete_legacy_completion_states_terminalize_deterministically() {
    for (state, provider_id) in [
        ("answer-ready", "legacy-answer-ready"),
        ("delivering", "legacy-delivering"),
    ] {
        let fixture = accepted_run(provider_id);
        fixture
            .db
            .conn
            .execute(
                "UPDATE receiver_jobs SET state = ?2 WHERE job_id = ?1",
                rusqlite::params![fixture.job_id.to_string(), state],
            )
            .expect("simulate incomplete legacy completion row");
        let effect = fixture
            .db
            .reconcile_next_receiver_job(1_500)
            .expect("reconcile legacy completion")
            .expect("legacy terminal effect");
        assert_eq!(effect.action(), ReceiverReconciliationAction::TerminalFailure);
        assert_eq!(
            effect.reason(),
            ReceiverReconciliationReason::IncompleteLegacyCompletion
        );
        let terminal = fixture
            .db
            .receiver_job(fixture.job_id)
            .expect("load legacy terminal job")
            .expect("legacy terminal job");
        assert_eq!(terminal.state(), ReceiverJobState::Failed);
    }
}

#[test]
fn semantic_notice_delivery_rows_leave_their_source_jobs_to_the_delivery_lane() {
    for state in ["answer-ready", "delivering"] {
        let fixture = accepted_run(&format!("notice-only-{state}"));
        fixture
            .db
            .conn
            .execute(
                "UPDATE receiver_jobs SET state = ?2 WHERE job_id = ?1",
                rusqlite::params![fixture.job_id.to_string(), state],
            )
            .expect("stage incomplete final-answer state");
        let token = fixture
            .db
            .receiver_job(fixture.job_id)
            .expect("load staged job")
            .expect("durable job")
            .token();
        let notice = crate::server::reply::unanswered_notice("sms");
        assert!(
            super::super::store::response_intent::insert(
                &fixture.db.conn,
                fixture.job_id,
                token,
                &fixture.inbound,
                ReceiverResponseKind::UnavailableNotice,
                &notice.text,
                1,
            )
            .expect("stage semantic notice delivery")
        );

        assert!(
            fixture
                .db
                .reconcile_next_receiver_job(1_500)
                .expect("reconcile semantic notice source")
                .is_none(),
            "semantic response authority belongs only to the delivery lane"
        );
        assert_eq!(
            fixture
                .db
                .receiver_job(fixture.job_id)
                .expect("load semantic notice source")
                .expect("semantic notice source")
                .state()
                .as_str(),
            state
        );
    }
}
