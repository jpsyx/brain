#[test]
fn reconciliation_winner_fences_every_late_writer_from_the_old_instance() {
    let fixture = accepted_run("late-writer-fence");
    let token = fixture.ordinary.token();
    let conversation_id = fixture.ordinary.conversation_id();
    fixture
        .db
        .reconcile_next_receiver_job(301_400)
        .expect("reconcile stalled work")
        .expect("recovery effect");
    let after_reconciliation = fixture
        .db
        .receiver_job(fixture.job_id)
        .expect("load reconciled job")
        .expect("reconciled job");
    let scope = crate::agent::SessionScope::new(
        crate::agent::AgentKind::Codex,
        fixture.inbound.workspace_id,
        fixture.inbound.actor.clone(),
    );
    let session = crate::agent::AgentSession::new("native-session").expect("native session");
    let old_registration = ReceiverSessionAttribution::new(
        conversation_id,
        "ordinary-instance".to_owned(),
        session.clone(),
        scope,
    );
    assert!(
        !fixture
            .db
            .apply_receiver_observation(
                fixture.job_id,
                "ordinary-owner",
                &observation(
                    token,
                    "ordinary-instance",
                    "native-session",
                    ReceiverNonterminalObservationPhase::Progressing,
                    3,
                    301_401,
                ),
            )
            .expect("reject late progress")
    );
    assert!(
        !fixture
            .db
            .complete_receiver_job_with_binding(&ReceiverCompletionRequest {
                job_id: fixture.job_id,
                token,
                owner: "ordinary-owner",
                registration: &old_registration,
                completed_session: &session,
                observed_at_unix_ms: 301_401,
                authorized_at_unix_ms: 301_401,
            })
            .expect("reject late completion")
    );
    assert!(
        !fixture
            .db
            .renew_receiver_claim(fixture.job_id, "ordinary-owner", 301_401, 331_401)
            .expect("reject late renewal")
    );
    assert!(
        fixture
            .db
            .record_receiver_launch_retry(
                fixture.job_id,
                "ordinary-owner",
                301_401,
                306_401,
                ReceiverLaunchFailure::Spawn,
            )
            .expect("reject late process exit")
            .is_none()
    );
    assert_eq!(
        fixture
            .db
            .receiver_job(fixture.job_id)
            .expect("reload fenced job")
            .expect("fenced job"),
        after_reconciliation
    );
}

#[test]
fn two_reconcilers_publish_only_one_effect_for_the_same_snapshot() {
    let temporary = tempfile::tempdir().expect("temporary receiver state");
    let path = temporary.path().join("state.db");
    let fixture = accepted_run_in(
        Db::open_path_with_legacy_identity(
            &path,
            &receiver_workspace_id().to_string(),
            receiver_user_id().as_str(),
        )
        .expect("open first receiver store"),
        "two-reconcilers",
    );
    let second = Db::open_path_with_legacy_identity(
        &path,
        &receiver_workspace_id().to_string(),
        receiver_user_id().as_str(),
    )
    .expect("open second receiver store");
    assert!(
        fixture
            .db
            .reconcile_next_receiver_job(301_400)
            .expect("first reconciliation")
            .is_some()
    );
    assert!(
        second
            .reconcile_next_receiver_job(301_400)
            .expect("second reconciliation")
            .is_none()
    );
    let reconciled = second
        .receiver_job(fixture.job_id)
        .expect("load reconciled job")
        .expect("reconciled job");
    assert_eq!(reconciled.recovery_count(), 1);
    assert_eq!(reconciled.state(), ReceiverJobState::Retrying);
}

#[test]
fn immediate_writer_lock_serializes_real_separate_handle_reconcilers() {
    let temporary = tempfile::tempdir().expect("temporary receiver state");
    let path = temporary.path().join("state.db");
    let fixture = accepted_run_in(
        Db::open_path_with_legacy_identity(
            &path,
            &receiver_workspace_id().to_string(),
            receiver_user_id().as_str(),
        )
        .expect("open first receiver store"),
        "real-reconciler-lock",
    );
    let second = Db::open_path_with_legacy_identity(
        &path,
        &receiver_workspace_id().to_string(),
        receiver_user_id().as_str(),
    )
    .expect("open second receiver store");
    second
        .conn
        .busy_timeout(std::time::Duration::ZERO)
        .expect("disable second-handle busy wait");
    let first_writer = rusqlite::Transaction::new_unchecked(
        &fixture.db.conn,
        rusqlite::TransactionBehavior::Immediate,
    )
    .expect("hold first immediate writer");
    let start = std::sync::Arc::new(std::sync::Barrier::new(2));
    let second_start = std::sync::Arc::clone(&start);
    let (second, blocked) = std::thread::scope(|scope| {
        let contender = scope.spawn(move || {
            second_start.wait();
            let blocked = second.reconcile_next_receiver_job(301_400);
            (second, blocked)
        });
        start.wait();
        contender.join().expect("join second reconciler")
    });
    let blocked = blocked.expect_err("second immediate writer must observe the held lock");
    assert!(
        blocked.to_string().contains("database is locked"),
        "unexpected SQLite lock error: {blocked:#}"
    );
    drop(first_writer);

    assert!(
        second
            .reconcile_next_receiver_job(301_400)
            .expect("retry second reconciler after first rollback")
            .is_some()
    );
    assert_eq!(
        second
            .receiver_job(fixture.job_id)
            .expect("load reconciled job")
            .expect("reconciled job")
            .recovery_count(),
        1
    );
}

#[test]
fn exact_completion_winning_first_defeats_reconciliation() {
    let fixture = accepted_run("completion-wins-race");
    fixture
        .db
        .conn
        .execute(
            "UPDATE receiver_jobs SET claim_expires_at_unix_ms = 400000
             WHERE job_id = ?1",
            [fixture.job_id.to_string()],
        )
        .expect("keep exact completion owner live at the boundary");
    fixture
        .db
        .conn
        .execute(
            "UPDATE brain_sessions SET completion_status = 'completed'
             WHERE agent_session_id = 'native-session'",
            [],
        )
        .expect("record exact native completion");
    let binding = ReceiverSessionBinding::new(crate::agent::AgentKind::Codex, "native-session")
        .expect("native binding");
    assert!(
        fixture
            .db
            .update_receiver_conversation(
                fixture.ordinary.conversation_id(),
                "",
                Some(&binding),
                301_399,
            )
            .expect("persist exact completion binding")
    );
    let scope = crate::agent::SessionScope::new(
        crate::agent::AgentKind::Codex,
        fixture.inbound.workspace_id,
        fixture.inbound.actor.clone(),
    );
    let session = crate::agent::AgentSession::new("native-session").expect("native session");
    let registration = ReceiverSessionAttribution::new(
        fixture.ordinary.conversation_id(),
        "ordinary-instance".to_owned(),
        session.clone(),
        scope,
    );
    assert!(
        fixture
            .db
            .complete_receiver_job_with_binding(&ReceiverCompletionRequest {
                job_id: fixture.job_id,
                token: fixture.ordinary.token(),
                owner: "ordinary-owner",
                registration: &registration,
                completed_session: &session,
                observed_at_unix_ms: 301_400,
                authorized_at_unix_ms: 301_400,
            })
            .expect("exact completion wins")
    );
    assert!(
        fixture
            .db
            .reconcile_next_receiver_job(301_400)
            .expect("completion defeats reconciliation")
            .is_none()
    );
    assert_eq!(
        fixture
            .db
            .receiver_job(fixture.job_id)
            .expect("load completed job")
            .expect("completed job")
            .state(),
        ReceiverJobState::Done
    );
}
