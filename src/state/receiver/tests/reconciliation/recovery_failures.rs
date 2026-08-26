fn claimed_recovery(provider_id: &str) -> AcceptedRunFixture {
    let fixture = accepted_run(provider_id);
    fixture
        .db
        .reconcile_next_receiver_job(301_400)
        .expect("persist due recovery")
        .expect("recovery effect");
    acknowledge_accepted_run_cleanup(&fixture, 301_401);
    fixture
        .db
        .claim_receiver_recovery_run(fixture.job_id, "recovery-owner", 301_402, 331_402)
        .expect("claim due recovery")
        .expect("recovery claim");
    fixture
}

#[test]
fn ordinary_launch_retry_seam_rejects_every_recovery_failure_category() {
    for failure in ReceiverLaunchFailure::ALL {
        let fixture = claimed_recovery(&format!("ordinary-retry-bypass-{failure:?}"));
        let observed_at = if failure == ReceiverLaunchFailure::Spawn {
            assert!(
                fixture
                    .db
                    .prepare_receiver_job_launch(fixture.job_id, "recovery-owner", 301_403)
                    .expect("prepare recovery spawn")
            );
            301_404
        } else {
            301_403
        };
        let before = fixture
            .db
            .receiver_job(fixture.job_id)
            .expect("load recovery before ordinary retry seam")
            .expect("recovery before ordinary retry seam");
        assert!(
            fixture
                .db
                .record_receiver_launch_retry(
                    fixture.job_id,
                    "recovery-owner",
                    observed_at,
                    observed_at + 5_000,
                    failure,
                )
                .expect("ordinary retry seam rejects recovery")
                .is_none(),
            "ordinary retry seam accepted {failure:?} for recovery"
        );
        assert_eq!(
            fixture
                .db
                .receiver_job(fixture.job_id)
                .expect("reload rejected recovery")
                .expect("rejected recovery"),
            before
        );
    }
}

#[test]
fn every_claimed_recovery_launch_failure_terminalizes_with_notice_intent() {
    for (failure, reason) in [
        (
            ReceiverRecoveryFailure::Planning,
            ReceiverReconciliationReason::RecoveryPlanningFailed,
        ),
        (
            ReceiverRecoveryFailure::Registration,
            ReceiverReconciliationReason::RecoveryRegistrationFailed,
        ),
        (
            ReceiverRecoveryFailure::Spawn,
            ReceiverReconciliationReason::RecoverySpawnFailed,
        ),
        (
            ReceiverRecoveryFailure::Shutdown,
            ReceiverReconciliationReason::RecoveryShutdown,
        ),
    ] {
        let fixture = claimed_recovery(&format!("terminal-recovery-{failure:?}"));
        if failure == ReceiverRecoveryFailure::Spawn {
            assert!(
                fixture
                    .db
                    .prepare_receiver_job_launch(fixture.job_id, "recovery-owner", 301_403)
                    .expect("prepare recovery before spawn failure")
            );
        }
        let before = fixture
            .db
            .receiver_job(fixture.job_id)
            .expect("load recovery before terminal failure")
            .expect("recovery before terminal failure");
        let effect = fixture
            .db
            .fail_receiver_recovery_attempt(
                fixture.job_id,
                "recovery-owner",
                301_404,
                failure,
            )
            .expect("record exact recovery failure")
            .expect("terminal recovery effect");
        assert_eq!(effect.action(), ReceiverReconciliationAction::TerminalFailure);
        assert_eq!(effect.reason(), reason);
        let terminal = fixture
            .db
            .receiver_job(fixture.job_id)
            .expect("load terminal recovery")
            .expect("terminal recovery");
        assert_eq!(terminal.state(), ReceiverJobState::Failed);
        assert_eq!(terminal.retry_count(), before.retry_count());
        assert_eq!(terminal.recovery_count(), 1);
        assert_eq!(terminal.last_error(), Some(reason.as_str()));
        assert!(terminal.pending_unavailable_notice());
    }
}
