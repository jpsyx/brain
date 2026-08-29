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

#[test]
fn active_final_answer_does_not_block_a_later_due_recovery_claim() {
    let target = stalled_run("recovery-behind-final-answer");
    target
        .db
        .reconcile_next_receiver_job(301_400)
        .expect("reconcile target recovery")
        .expect("target recovery effect");
    acknowledge_stalled_cleanup(&target, 301_401);
    let target_job_id = target.job_id;
    let target_token = target.ordinary.token();
    let older_job = receiver_job(Some("older-final-answer"), 50);
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let older = super::binding::completion_fixture_for_job(
        target.db,
        ReceiverJobState::Processing,
        older_job,
        &identity,
    );
    older
        .db
        .complete_receiver_job_with_binding(&older.request())
        .expect("record older final answer")
        .expect("exact older answer owner");

    let recovery = older
        .db
        .claim_next_receiver_recovery_run("recovery-owner", 302_100, 332_100)
        .expect("discover due recovery behind active delivery")
        .expect("active final answer must not block recovery");

    assert!(recovery.job().id() == target_job_id);
    assert!(recovery.job().token() == target_token);
    assert!(recovery.claim().owner() == "recovery-owner");
    assert!(
        older
            .db
            .receiver_job(older.job_id)
            .expect("load older final-answer job")
            .is_some_and(|job| job.state() == ReceiverJobState::AnswerReady),
        "the independent final-answer delivery left its active state"
    );
    let delivery_is_exact_and_ready: bool = older
        .db
        .conn
        .query_row(
            "SELECT EXISTS(
               SELECT 1 FROM receiver_deliveries
               WHERE job_id = ?1 AND job_token = ?2
                 AND response_kind = 'final-answer' AND state = 'ready'
             )",
            rusqlite::params![older.job_id.to_string(), older.token.to_string()],
            |row| row.get(0),
        )
        .expect("inspect older final-answer fence");
    assert!(
        delivery_is_exact_and_ready,
        "recovery claim changed the independent exact delivery"
    );
}
