struct ExactSpawnedRecoveryFixture {
    db: Db,
    job_id: ReceiverJobId,
    token: ReceiverJobToken,
    conversation_id: ReceiverConversationId,
    inbound: crate::server::receiver::InboundJob,
    registration: ReceiverSessionAttribution,
    later: ReceiverAcceptance,
}

fn exact_spawned_recovery_in(db: Db, provider_id: &str) -> ExactSpawnedRecoveryFixture {
    let accepted = accepted_run_in(db, provider_id);
    accepted
        .db
        .reconcile_next_receiver_job(301_400)
        .expect("persist due recovery")
        .expect("recovery effect");
    acknowledge_accepted_run_cleanup(&accepted, 301_401);
    accepted
        .db
        .claim_receiver_recovery_run(accepted.job_id, "recovery-owner", 301_402, 331_402)
        .expect("claim recovery")
        .expect("recovery claim");
    let scope = crate::agent::SessionScope::new(
        crate::agent::AgentKind::Codex,
        accepted.inbound.workspace_id,
        accepted.inbound.actor.clone(),
    );
    let session = crate::agent::AgentSession::new("native-session").expect("native session");
    let registration = accepted
        .db
        .claim_receiver_session(
            accepted.ordinary.conversation_id(),
            &session,
            "recovery-owner",
            42,
            &scope,
        )
        .expect("claim exact recovery session")
        .expect("exact recovery registration");
    accepted
        .db
        .conn
        .execute(
            "UPDATE brain_sessions SET source = 'startup'
             WHERE brain_instance_id = 'recovery-owner'",
            [],
        )
        .expect("record recovery lifecycle source");
    assert!(
        accepted
            .db
            .prepare_receiver_recovery_job_launch(
                accepted.job_id,
                "recovery-owner",
                301_403,
            )
            .expect("prepare recovery launch")
    );
    let later = accepted
        .db
        .accept_receiver_job(
            &receiver_job(Some(&format!("{provider_id}-later")), 200),
            &ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id()),
        )
        .expect("accept later FIFO work");
    ExactSpawnedRecoveryFixture {
        db: accepted.db,
        job_id: accepted.job_id,
        token: accepted.ordinary.token(),
        conversation_id: accepted.ordinary.conversation_id(),
        inbound: accepted.inbound,
        registration,
        later,
    }
}

fn exact_spawned_recovery(provider_id: &str) -> ExactSpawnedRecoveryFixture {
    exact_spawned_recovery_in(
        Db::open_in_memory().expect("receiver state"),
        provider_id,
    )
}

#[test]
fn spawned_cleanup_terminalizes_after_claim_expiry_with_typed_exact_outcomes() {
    let fixture = exact_spawned_recovery("typed-expired-cleanup");
    let unrelated_session =
        crate::agent::AgentSession::new("unrelated-session").expect("unrelated session");
    fixture
        .db
        .register_receiver_session(
            fixture.conversation_id,
            &unrelated_session,
            "unrelated-instance",
            88,
            fixture.registration.scope(),
        )
        .expect("register unrelated receiver session");
    let mut first = None;
    let records = crate::logging::capture_receiver_lifecycle(|| {
        first = Some(
            fixture
                .db
                .establish_receiver_spawned_recovery_cleanup(
                    fixture.job_id,
                    fixture.token,
                    "recovery-owner",
                    &fixture.registration,
                    42,
                    331_403,
                )
                .expect("terminalize expired spawned recovery"),
        );
    });
    assert_receiver_lifecycle_records(
        &records,
        &[
            "receiver lifecycle event=terminal-advancement phase=failed queue_depth=1 reason=recovery-shutdown"
        ],
    );
    let first = first.expect("captured recovery cleanup outcome");
    let ReceiverRecoveryCleanupOutcome::Exact(effect) = first else {
        panic!("expired exact recovery was classified as changed");
    };
    assert_eq!(effect.reason(), ReceiverReconciliationReason::RecoveryShutdown);
    assert_eq!(effect.cleanup_instance(), Some("recovery-owner"));
    assert_eq!(effect.cleanup_session_id(), Some("native-session"));
    let second = fixture
        .db
        .establish_receiver_spawned_recovery_cleanup(
            fixture.job_id,
            fixture.token,
            "recovery-owner",
            &fixture.registration,
            42,
            331_404,
        )
        .expect("redrive already terminal exact recovery");
    assert_eq!(second, ReceiverRecoveryCleanupOutcome::Exact(effect));
    assert!(
        !fixture
            .db
            .acknowledge_receiver_recovery_cleanup(
                fixture.job_id,
                ReceiverJobToken::new(),
                "recovery-owner",
                "native-session",
                331_405,
            )
            .expect("reject wrong cleanup token")
    );
    assert!(
        fixture
            .db
            .acknowledge_receiver_recovery_cleanup(
                fixture.job_id,
                fixture.token,
                "recovery-owner",
                "native-session",
                331_406,
            )
            .expect("acknowledge exact spawned cleanup")
    );
    let unrelated_lock = fixture
        .db
        .conn
        .query_row(
            "SELECT locked_pid FROM brain_sessions
             WHERE brain_instance_id = 'unrelated-instance'",
            [],
            |row| row.get::<_, Option<i64>>(0),
        )
        .expect("load unrelated session lock");
    assert_eq!(unrelated_lock, Some(88));
    let unrelated_registration = fixture
        .db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM receiver_session_registrations
             WHERE brain_instance_id = 'unrelated-instance'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("count unrelated registration");
    assert_eq!(unrelated_registration, 1);
    assert!(
        fixture
            .db
            .reconcile_next_receiver_job(331_407)
            .expect("cleanup no longer redrives")
            .is_none()
    );
    let next = fixture
        .db
        .claim_next_receiver_run("later-owner", 331_407, 361_407)
        .expect("claim later FIFO work")
        .expect("later FIFO work is immediately eligible");
    assert_eq!(next.job().id(), fixture.later.job_id());
}

#[test]
fn spawned_cleanup_rejects_wrong_identity_without_releasing_exact_registration() {
    let fixture = exact_spawned_recovery("typed-cleanup-mismatch");
    let other = fixture
        .db
        .receiver_job(fixture.later.job_id())
        .expect("load unrelated job")
        .expect("unrelated job");
    let wrong_session = crate::agent::AgentSession::new("wrong-session").expect("wrong session");
    let wrong_frontend_scope = crate::agent::SessionScope::new(
        crate::agent::AgentKind::Claude,
        fixture.inbound.workspace_id,
        fixture.inbound.actor.clone(),
    );
    let wrong_frontend = ReceiverSessionAttribution::new(
        fixture.conversation_id,
        "recovery-owner".to_owned(),
        fixture.registration.registered_session().clone(),
        wrong_frontend_scope,
    );
    let wrong_conversation = ReceiverSessionAttribution::new(
        ReceiverConversationId::new(),
        "recovery-owner".to_owned(),
        fixture.registration.registered_session().clone(),
        fixture.registration.scope().clone(),
    );
    let wrong_instance = ReceiverSessionAttribution::new(
        fixture.conversation_id,
        "wrong-instance".to_owned(),
        fixture.registration.registered_session().clone(),
        fixture.registration.scope().clone(),
    );
    let wrong_session = ReceiverSessionAttribution::new(
        fixture.conversation_id,
        "recovery-owner".to_owned(),
        wrong_session,
        fixture.registration.scope().clone(),
    );
    for (label, job_id, token, owner, registration, pid) in [
        ("job",
            fixture.later.job_id(),
            fixture.token,
            "recovery-owner",
            &fixture.registration,
            42,
        ),
        ("token",
            fixture.job_id,
            other.token(),
            "recovery-owner",
            &fixture.registration,
            42,
        ),
        ("conversation",
            fixture.job_id,
            fixture.token,
            "recovery-owner",
            &wrong_conversation,
            42,
        ),
        ("instance",
            fixture.job_id,
            fixture.token,
            "wrong-instance",
            &wrong_instance,
            42,
        ),
        ("session",
            fixture.job_id,
            fixture.token,
            "recovery-owner",
            &wrong_session,
            42,
        ),
        ("frontend",
            fixture.job_id,
            fixture.token,
            "recovery-owner",
            &wrong_frontend,
            42,
        ),
        ("pid",
            fixture.job_id,
            fixture.token,
            "recovery-owner",
            &fixture.registration,
            99,
        ),
    ] {
        assert_eq!(
            fixture
                .db
                .establish_receiver_spawned_recovery_cleanup(
                    job_id,
                    token,
                    owner,
                    registration,
                    pid,
                    331_403,
                )
                .expect("classify mismatched cleanup"),
            ReceiverRecoveryCleanupOutcome::Changed,
            "wrong {label} was accepted"
        );
    }
    let locked_pid = fixture
        .db
        .conn
        .query_row(
            "SELECT locked_pid FROM brain_sessions
             WHERE brain_instance_id = 'recovery-owner'",
            [],
            |row| row.get::<_, Option<i64>>(0),
        )
        .expect("load exact recovery lock");
    assert_eq!(locked_pid, Some(42));
    assert_eq!(
        fixture
            .db
            .receiver_job(fixture.job_id)
            .expect("load unchanged recovery")
            .expect("unchanged recovery")
            .state(),
        ReceiverJobState::Launching
    );
}

#[test]
fn preobservation_cleanup_restart_requires_the_exact_dead_pid_and_acknowledgement() {
    let temporary = tempfile::tempdir().expect("temporary receiver state");
    let path = temporary.path().join("state.db");
    let fixture = exact_spawned_recovery_in(
        Db::open_path_with_legacy_identity(
            &path,
            &receiver_workspace_id().to_string(),
            receiver_user_id().as_str(),
        )
        .expect("open initial receiver state"),
        "preobservation-restart",
    );
    let effect = fixture
        .db
        .reconcile_next_receiver_job(421_403)
        .expect("terminalize preobservation recovery")
        .expect("exact preobservation effect");
    assert_eq!(effect.cleanup_instance(), Some("recovery-owner"));
    assert_eq!(effect.cleanup_session_id(), Some("native-session"));
    let job_id = fixture.job_id;
    let token = fixture.token;
    drop(fixture);

    let live = Db::open_path_with_legacy_identity(
        &path,
        &receiver_workspace_id().to_string(),
        receiver_user_id().as_str(),
    )
    .expect("reopen with live cleanup owner")
    .with_pid_alive(|pid| pid == 42);
    assert!(
        !live
            .receiver_cleanup_registration_is_stale(&effect)
            .expect("reject live cleanup PID")
    );
    drop(live);

    let dead = Db::open_path_with_legacy_identity(
        &path,
        &receiver_workspace_id().to_string(),
        receiver_user_id().as_str(),
    )
    .expect("reopen with dead cleanup owner")
    .with_pid_alive(|_| false);
    assert!(
        dead.receiver_cleanup_registration_is_stale(&effect)
            .expect("accept exact dead cleanup PID")
    );
    assert!(
        !dead
            .acknowledge_receiver_recovery_cleanup(
                job_id,
                token,
                "wrong-instance",
                "native-session",
                421_404,
            )
            .expect("reject wrong restart acknowledgement")
    );
    assert!(
        dead.acknowledge_receiver_recovery_cleanup(
            job_id,
            token,
            "recovery-owner",
            "native-session",
            421_405,
        )
        .expect("acknowledge exact dead restart cleanup")
    );
}

#[test]
fn concurrent_reconciliation_and_local_cleanup_share_one_exact_tuple() {
    let temporary = tempfile::tempdir().expect("temporary receiver state");
    let path = temporary.path().join("state.db");
    let fixture = exact_spawned_recovery_in(
        Db::open_path_with_legacy_identity(
            &path,
            &receiver_workspace_id().to_string(),
            receiver_user_id().as_str(),
        )
        .expect("open reconciliation handle"),
        "spawned-cleanup-race",
    );
    let local = Db::open_path_with_legacy_identity(
        &path,
        &receiver_workspace_id().to_string(),
        receiver_user_id().as_str(),
    )
    .expect("open local cleanup handle");
    let start = std::sync::Arc::new(std::sync::Barrier::new(2));
    let other_start = std::sync::Arc::clone(&start);
    let registration = fixture.registration.clone();
    let job_id = fixture.job_id;
    let token = fixture.token;
    let (reconciled, local_outcome) = std::thread::scope(|scope| {
        let local_cleanup = scope.spawn(move || {
            other_start.wait();
            local.establish_receiver_spawned_recovery_cleanup(
                job_id,
                token,
                "recovery-owner",
                &registration,
                42,
                421_403,
            )
        });
        start.wait();
        let reconciled = fixture.db.reconcile_next_receiver_job(421_403);
        (
            reconciled,
            local_cleanup.join().expect("join local cleanup writer"),
        )
    });
    let reconciled = reconciled
        .expect("concurrent reconciliation")
        .expect("reconciled cleanup effect");
    let ReceiverRecoveryCleanupOutcome::Exact(local_effect) =
        local_outcome.expect("concurrent local cleanup")
    else {
        panic!("serialized local cleanup did not recover the exact tuple");
    };
    assert_eq!(local_effect, reconciled);
    assert!(
        fixture
            .db
            .acknowledge_receiver_recovery_cleanup(
                fixture.job_id,
                fixture.token,
                "recovery-owner",
                "native-session",
                421_404,
            )
            .expect("one exact cleanup acknowledgement")
    );
    assert!(
        !fixture
            .db
            .acknowledge_receiver_recovery_cleanup(
                fixture.job_id,
                fixture.token,
                "recovery-owner",
                "native-session",
                421_405,
            )
            .expect("second acknowledgement has no authority")
    );
    assert!(
        fixture
            .db
            .reconcile_next_receiver_job(421_406)
            .expect("cleanup no longer recurs")
            .is_none()
    );
}
