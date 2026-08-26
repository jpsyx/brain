#[derive(Clone, Copy)]
enum OlderRecoveryBlocker {
    ExpiredOwner,
    DueOrdinaryRetry,
}

fn due_recovery_behind_older_blocker(
    provider_suffix: &str,
    blocker: OlderRecoveryBlocker,
) -> (Db, ReceiverJobId) {
    let fixture = stalled_run(&format!("ordered-recovery-{provider_suffix}"));
    fixture
        .db
        .reconcile_next_receiver_job(301_400)
        .expect("reconcile target recovery")
        .expect("target recovery effect");
    acknowledge_stalled_cleanup(&fixture, 301_401);
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let older = fixture
        .db
        .accept_receiver_job(
            &receiver_job(Some(&format!("older-{provider_suffix}")), 50),
            &identity,
        )
        .expect("accept older FIFO job");
    fixture
        .db
        .claim_next_receiver_run("older-owner", 302_000, 302_100)
        .expect("claim older FIFO job")
        .expect("older FIFO claim");
    if matches!(blocker, OlderRecoveryBlocker::DueOrdinaryRetry) {
        assert_eq!(
            fixture
                .db
                .record_receiver_launch_retry(
                    older.job_id(),
                    "older-owner",
                    302_050,
                    302_100,
                    ReceiverLaunchFailure::Planning,
                )
                .expect("schedule older ordinary retry"),
            Some(ReceiverLaunchRetryOutcome::Scheduled)
        );
    }
    (fixture.db, fixture.job_id)
}

#[test]
fn targeted_recovery_claim_refuses_every_older_workspace_blocker() {
    for (suffix, blocker) in [
        ("targeted-expired", OlderRecoveryBlocker::ExpiredOwner),
        ("targeted-retry", OlderRecoveryBlocker::DueOrdinaryRetry),
    ] {
        let (db, target) = due_recovery_behind_older_blocker(suffix, blocker);
        assert!(
            db.claim_receiver_recovery_run(target, "recovery-owner", 302_100, 332_100)
                .expect("gate targeted recovery by global FIFO")
                .is_none(),
            "targeted recovery skipped an older blocker for {suffix}"
        );
    }
}

#[test]
fn recovery_discovery_refuses_every_older_workspace_blocker() {
    for (suffix, blocker) in [
        ("discovery-expired", OlderRecoveryBlocker::ExpiredOwner),
        ("discovery-retry", OlderRecoveryBlocker::DueOrdinaryRetry),
    ] {
        let (db, _) = due_recovery_behind_older_blocker(suffix, blocker);
        assert!(
            db.claim_next_receiver_recovery_run("recovery-owner", 302_100, 332_100)
                .expect("gate recovery discovery by global FIFO")
                .is_none(),
            "recovery discovery skipped an older blocker for {suffix}"
        );
    }
}
