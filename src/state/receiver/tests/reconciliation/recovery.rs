#[test]
fn reconciliation_persists_one_ownerless_same_session_recovery_before_claim() {
    let fixture = accepted_run("persist-recovery-before-claim");
    assert!(
        fixture
            .db
            .claim_receiver_recovery_run(fixture.job_id, "recovery-owner", 301_400, 331_400)
            .expect("direct stale-work claim is rejected")
            .is_none()
    );
    let effect = fixture
        .db
        .reconcile_next_receiver_job(301_400)
        .expect("reconcile exact progress expiry")
        .expect("persist recovery effect");
    assert_eq!(
        effect.action(),
        ReceiverReconciliationAction::ScheduleRecovery
    );
    assert_eq!(effect.reason(), ReceiverReconciliationReason::AcceptedStall);
    assert_eq!(effect.cleanup_instance(), Some("ordinary-instance"));
    let due = fixture
        .db
        .receiver_job(fixture.job_id)
        .expect("load due recovery")
        .expect("due recovery");
    assert_eq!(due.state(), ReceiverJobState::Retrying);
    assert_eq!(due.id(), fixture.ordinary.id());
    assert_eq!(due.token(), fixture.ordinary.token());
    assert_eq!(due.conversation_id(), fixture.ordinary.conversation_id());
    assert_eq!(due.inbound(), &fixture.inbound);
    assert_eq!(due.retry_at_unix_ms(), Some(301_400));
    assert_eq!(due.retry_from_state(), Some(ReceiverJobState::Processing));
    assert_eq!(due.recovery_count(), 1);
    assert_eq!(due.attempt_kind(), ReceiverAttemptKind::Recovery);
    assert_eq!(due.observation_instance(), None);
    assert_eq!(due.observation_session_id(), None);
    assert_eq!(due.observation_revision(), 0);
    assert_eq!(due.attempt_accepted_at_unix_ms(), None);
    assert_eq!(due.attempt_progressing_at_unix_ms(), None);
    assert_eq!(due.latest_progress_at_unix_ms(), None);
    assert_eq!(due.launch_expires_at_unix_ms(), None);
    assert_eq!(due.acceptance_expires_at_unix_ms(), None);
    assert_eq!(due.progress_expires_at_unix_ms(), None);
    assert_eq!(due.recovery_expires_at_unix_ms(), Some(601_400));
    assert_eq!(
        due.absolute_work_expires_at_unix_ms(),
        fixture.ordinary.absolute_work_expires_at_unix_ms()
    );
    assert_eq!(
        fixture
            .db
            .receiver_conversation(due.conversation_id())
            .expect("load recovery conversation")
            .expect("recovery conversation")
            .session_plan(crate::agent::AgentKind::Codex),
        ReceiverSessionPlan::ResumeNative("native-session".to_owned())
    );
    assert!(
        fixture
            .db
            .claim_next_receiver_run("ordinary-owner", 301_400, 331_400)
            .expect("ordinary claim cannot consume due recovery")
            .is_none()
    );
    let recovery = fixture
        .db
        .claim_receiver_recovery_run(fixture.job_id, "recovery-owner", 301_400, 331_400)
        .expect("claim persisted recovery")
        .expect("persisted recovery run");
    assert_eq!(recovery.job().state(), ReceiverJobState::Claimed);
    assert_eq!(recovery.job().recovery_count(), 1);
    assert_eq!(recovery.job().attempt_kind(), ReceiverAttemptKind::Recovery);
    assert_eq!(recovery.job().launch_expires_at_unix_ms(), Some(421_400));
}

#[test]
fn due_recovery_survives_reopen_and_is_discovered_before_later_fifo_work() {
    let temporary = tempfile::tempdir().expect("temporary receiver state");
    let path = temporary.path().join("state.db");
    let fixture = accepted_run_in(
        Db::open_path_with_legacy_identity(
            &path,
            &receiver_workspace_id().to_string(),
            receiver_user_id().as_str(),
        )
        .expect("open receiver state"),
        "restart-recovery",
    );
    let later = receiver_job(Some("restart-later-work"), 200);
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    fixture
        .db
        .accept_receiver_job(&later, &identity)
        .expect("accept later job");
    let job_id = fixture.job_id;
    drop(fixture);
    let waiting = Db::open_path_with_legacy_identity(
        &path,
        &receiver_workspace_id().to_string(),
        receiver_user_id().as_str(),
    )
    .expect("reopen below recovery boundary");
    assert!(
        waiting
            .reconcile_next_receiver_job(301_399)
            .expect("wait below recovery boundary after reopen")
            .is_none()
    );
    drop(waiting);
    let reconciler = Db::open_path_with_legacy_identity(
        &path,
        &receiver_workspace_id().to_string(),
        receiver_user_id().as_str(),
    )
    .expect("reopen at recovery boundary");
    reconciler
        .reconcile_next_receiver_job(301_400)
        .expect("persist due recovery after reopen")
        .expect("recovery effect");
    drop(reconciler);
    let reopened = Db::open_path_with_legacy_identity(
        &path,
        &receiver_workspace_id().to_string(),
        receiver_user_id().as_str(),
    )
    .expect("reopen receiver state");
    let claim = reopened
        .claim_next_receiver_recovery_run("restart-owner", 301_400, 331_400)
        .expect("discover due recovery after reopen")
        .expect("due recovery claim");
    assert_eq!(claim.job().id(), job_id);
    assert_eq!(claim.job().attempt_kind(), ReceiverAttemptKind::Recovery);
    assert_eq!(claim.job().recovery_count(), 1);
}

#[test]
fn claimed_recovery_with_unsafe_native_history_terminalizes_durably() {
    let fixture = accepted_run("unsafe-native-history");
    fixture
        .db
        .reconcile_next_receiver_job(301_400)
        .expect("persist due recovery")
        .expect("recovery effect");
    fixture
        .db
        .claim_receiver_recovery_run(fixture.job_id, "recovery-owner", 301_400, 331_400)
        .expect("claim due recovery")
        .expect("recovery claim");
    let effect = fixture
        .db
        .fail_receiver_recovery_resume(fixture.job_id, "recovery-owner", 301_500)
        .expect("terminalize unsafe recovery resume")
        .expect("terminal recovery effect");
    assert_eq!(effect.action(), ReceiverReconciliationAction::TerminalFailure);
    assert_eq!(
        effect.reason(),
        ReceiverReconciliationReason::NativeSessionUnavailable
    );
    let terminal = fixture
        .db
        .receiver_job(fixture.job_id)
        .expect("load unsafe terminal job")
        .expect("unsafe terminal job");
    assert_eq!(terminal.state(), ReceiverJobState::Failed);
    assert_eq!(terminal.recovery_count(), 1);
    assert!(terminal.pending_unavailable_notice());
}

#[test]
fn accepted_work_waits_until_the_exact_progress_boundary() {
    let fixture = accepted_run("wait-before-progress-expiry");
    let before = fixture.ordinary.clone();

    assert!(
        fixture
            .db
            .reconcile_next_receiver_job(301_399)
            .expect("wait below exact progress expiry")
            .is_none()
    );
    assert_eq!(
        fixture
            .db
            .receiver_job(fixture.job_id)
            .expect("reload waiting job")
            .expect("waiting job"),
        before
    );
}

#[test]
fn accepted_recovery_stalling_again_terminalizes_at_its_bound() {
    let fixture = accepted_run("second-accepted-stall");
    fixture
        .db
        .reconcile_next_receiver_job(301_400)
        .expect("persist due recovery")
        .expect("recovery effect");
    fixture
        .db
        .claim_receiver_recovery_run(fixture.job_id, "recovery-owner", 301_400, 331_400)
        .expect("claim due recovery")
        .expect("recovery claim");
    let scope = crate::agent::SessionScope::new(
        crate::agent::AgentKind::Codex,
        fixture.inbound.workspace_id,
        fixture.inbound.actor.clone(),
    );
    let session = crate::agent::AgentSession::new("native-session").expect("native session");
    fixture
        .db
        .claim_receiver_session(
            fixture.ordinary.conversation_id(),
            &session,
            "recovery-instance",
            43,
            &scope,
        )
        .expect("claim exact recovery session")
        .expect("exact recovery registration");
    assert!(
        fixture
            .db
            .prepare_receiver_job_launch(fixture.job_id, "recovery-owner", 301_500)
            .expect("prepare recovery launch")
    );
    let token = fixture.ordinary.token();
    assert!(
        fixture
            .db
            .commit_receiver_job_launch(
                fixture.job_id,
                "recovery-owner",
                &launch_observation(token, "recovery-instance", "native-session", 301_600),
            )
            .expect("commit recovery launch")
    );
    assert!(
        fixture
            .db
            .apply_receiver_observation(
                fixture.job_id,
                "recovery-owner",
                &observation(
                    token,
                    "recovery-instance",
                    "native-session",
                    ReceiverNonterminalObservationPhase::Accepted,
                    1,
                    301_700,
                ),
            )
            .expect("commit recovery acceptance")
    );
    assert!(
        fixture
            .db
            .apply_receiver_observation(
                fixture.job_id,
                "recovery-owner",
                &observation(
                    token,
                    "recovery-instance",
                    "native-session",
                    ReceiverNonterminalObservationPhase::Progressing,
                    2,
                    301_800,
                ),
            )
            .expect("commit recovery progress")
    );

    let effect = fixture
        .db
        .reconcile_next_receiver_job(601_400)
        .expect("reconcile recovery expiry")
        .expect("terminal recovery effect");
    assert_eq!(effect.action(), ReceiverReconciliationAction::TerminalFailure);
    assert_eq!(effect.reason(), ReceiverReconciliationReason::RecoveryExpired);
    let terminal = fixture
        .db
        .receiver_job(fixture.job_id)
        .expect("load second-stall terminal job")
        .expect("second-stall terminal job");
    assert_eq!(terminal.state(), ReceiverJobState::Failed);
    assert_eq!(terminal.recovery_count(), 1);
    assert!(terminal.pending_unavailable_notice());
}
