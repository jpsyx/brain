#[test]
fn terminal_cleanup_survives_restart_redrives_and_acknowledges_exactly() {
    let temporary = tempfile::tempdir().expect("temporary receiver state");
    let path = temporary.path().join("state.db");
    let fixture = accepted_run_in(
        Db::open_path_with_legacy_identity(
            &path,
            &receiver_workspace_id().to_string(),
            receiver_user_id().as_str(),
        )
        .expect("open initial receiver state"),
        "terminal-cleanup-restart",
    );
    let later = fixture
        .db
        .accept_receiver_job(
            &receiver_job(Some("terminal-cleanup-later"), 200),
            &ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id()),
        )
        .expect("accept later FIFO work");
    fixture
        .db
        .reconcile_next_receiver_job(301_400)
        .expect("persist cleanup-pending recovery")
        .expect("schedule recovery effect");
    let job_id = fixture.job_id;
    let token = fixture.ordinary.token();
    let conversation_id = fixture.ordinary.conversation_id();
    drop(fixture);

    let terminal_store = Db::open_path_with_legacy_identity(
        &path,
        &receiver_workspace_id().to_string(),
        receiver_user_id().as_str(),
    )
    .expect("reopen at recovery expiry");
    let terminal_effect = terminal_store
        .reconcile_next_receiver_job(601_400)
        .expect("terminalize expired recovery")
        .expect("terminal cleanup effect");
    assert_eq!(
        terminal_effect.action(),
        ReceiverReconciliationAction::TerminalFailure
    );
    assert_eq!(
        terminal_effect.reason(),
        ReceiverReconciliationReason::RecoveryExpired
    );
    assert_eq!(terminal_effect.cleanup_instance(), Some("ordinary-instance"));
    assert_eq!(terminal_effect.cleanup_session_id(), Some("native-session"));
    assert_cleanup_pending(&terminal_store, job_id, conversation_id, true);

    let later_claim = terminal_store
        .claim_next_receiver_run("later-owner", 601_400, 631_400)
        .expect("claim later work while terminal cleanup is pending")
        .expect("later work is not FIFO-blocked");
    assert_eq!(later_claim.job().id(), later.job_id());
    drop(terminal_store);

    let redrive_store = Db::open_path_with_legacy_identity(
        &path,
        &receiver_workspace_id().to_string(),
        receiver_user_id().as_str(),
    )
    .expect("reopen pending terminal cleanup");
    let redriven = redrive_store
        .reconcile_next_receiver_job(601_401)
        .expect("redrive pending terminal cleanup")
        .expect("redriven cleanup effect");
    assert_eq!(redriven.reason(), ReceiverReconciliationReason::RecoveryExpired);
    assert_eq!(redriven.job_id(), job_id);
    assert_eq!(redriven.token(), token);
    assert_eq!(redriven.cleanup_instance(), Some("ordinary-instance"));
    assert_eq!(redriven.cleanup_session_id(), Some("native-session"));
    assert_cleanup_pending(&redrive_store, job_id, conversation_id, true);

    assert!(
        !redrive_store
            .acknowledge_receiver_recovery_cleanup(
                job_id,
                token,
                "wrong-instance",
                "native-session",
                601_402,
            )
            .expect("reject wrong terminal cleanup acknowledgement")
    );
    assert_cleanup_pending(&redrive_store, job_id, conversation_id, true);
    let cleanup_records = crate::logging::capture_receiver_lifecycle(|| {
        assert!(
            redrive_store
                .acknowledge_receiver_recovery_cleanup(
                    job_id,
                    token,
                    "ordinary-instance",
                    "native-session",
                    601_403,
                )
                .expect("acknowledge exact terminal cleanup")
        );
    });
    assert_receiver_lifecycle_records(
        &cleanup_records,
        &["receiver lifecycle event=cleanup-promotion delivery_phase=ready cleanup_gated=0"],
    );
    assert_cleanup_pending(&redrive_store, job_id, conversation_id, false);
    assert!(
        redrive_store
            .reconcile_next_receiver_job(601_404)
            .expect("terminal cleanup no longer redrives")
            .is_none()
    );
}

fn assert_cleanup_pending(
    db: &Db,
    job_id: ReceiverJobId,
    conversation_id: ReceiverConversationId,
    expected_pending: bool,
) {
    let job = db
        .receiver_job(job_id)
        .expect("load cleanup job")
        .expect("cleanup job");
    assert_eq!(
        job.state(),
        if expected_pending {
            ReceiverJobState::Failed
        } else {
            ReceiverJobState::AnswerReady
        }
    );
    assert_eq!(
        job.recovery_cleanup_instance(),
        expected_pending.then_some("ordinary-instance")
    );
    assert_eq!(
        job.recovery_cleanup_session_id(),
        expected_pending.then_some("native-session")
    );
    let registration_count = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM receiver_session_registrations
             WHERE workspace_id = ?1 AND conversation_id = ?2
               AND brain_instance_id = 'ordinary-instance'",
            rusqlite::params![
                receiver_workspace_id().to_string(),
                conversation_id.to_string(),
            ],
            |row| row.get::<_, i64>(0),
        )
        .expect("count cleanup registration");
    assert_eq!(registration_count, i64::from(expected_pending));
    let locked_pid = db
        .conn
        .query_row(
            "SELECT locked_pid FROM brain_sessions
             WHERE workspace_id = ?1 AND brain_instance_id = 'ordinary-instance'",
            [receiver_workspace_id().to_string()],
            |row| row.get::<_, Option<i64>>(0),
        )
        .expect("load cleanup session lock");
    assert_eq!(locked_pid, expected_pending.then_some(42));
    let delivery_state: String = db
        .conn
        .query_row(
            "SELECT state FROM receiver_deliveries
             WHERE job_id = ?1 AND response_kind = 'unavailable-notice'",
            [job_id.to_string()],
            |row| row.get(0),
        )
        .expect("load cleanup-gated unavailable response");
    assert_eq!(
        delivery_state,
        if expected_pending {
            "cleanup-gated"
        } else {
            "ready"
        }
    );
}
