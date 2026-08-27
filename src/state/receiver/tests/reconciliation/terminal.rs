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
    assert_eq!(terminal.state(), ReceiverJobState::Failed);
    assert_eq!(terminal.recovery_count(), 0);
    assert!(terminal.pending_unavailable_notice());
    assert_eq!(terminal.recovery_cleanup_instance(), None);
    assert_eq!(terminal.recovery_cleanup_session_id(), None);
    assert_eq!(
        terminal.last_error(),
        Some(ReceiverReconciliationReason::NativeSessionUnavailable.as_str())
    );
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
    assert!(terminal.pending_unavailable_notice());
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
    assert!(
        fixture
            .db
            .receiver_job(fixture.job_id)
            .expect("load absolute terminal job")
            .expect("absolute terminal job")
            .pending_unavailable_notice()
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
        assert!(terminal.pending_unavailable_notice());
    }
}
