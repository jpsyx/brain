#[test]
fn ownerless_due_recovery_terminalizes_at_recovery_expiry_after_reopen() {
    for (now, expected_reason) in [
        (601_399, None),
        (
            601_400,
            Some(ReceiverReconciliationReason::RecoveryExpired),
        ),
        (
            601_401,
            Some(ReceiverReconciliationReason::RecoveryExpired),
        ),
    ] {
        assert_reopened_due_recovery_boundary(now, false, expected_reason);
    }
}

#[test]
fn ownerless_due_recovery_terminalizes_at_absolute_expiry_after_reopen() {
    for (now, expected_reason) in [
        (1_801_299, None),
        (
            1_801_300,
            Some(ReceiverReconciliationReason::AbsoluteWorkExpired),
        ),
        (
            1_801_301,
            Some(ReceiverReconciliationReason::AbsoluteWorkExpired),
        ),
    ] {
        assert_reopened_due_recovery_boundary(now, true, expected_reason);
    }
}

fn assert_reopened_due_recovery_boundary(
    now_unix_ms: u64,
    extend_recovery_past_absolute: bool,
    expected_reason: Option<ReceiverReconciliationReason>,
) {
    let temporary = tempfile::tempdir().expect("temporary receiver state");
    let path = temporary.path().join("state.db");
    let fixture = accepted_run_in(
        Db::open_path_with_legacy_identity(
            &path,
            &receiver_workspace_id().to_string(),
            receiver_user_id().as_str(),
        )
        .expect("open receiver state"),
        &format!("ownerless-expiry-{now_unix_ms}-{extend_recovery_past_absolute}"),
    );
    fixture
        .db
        .reconcile_next_receiver_job(301_400)
        .expect("persist due recovery")
        .expect("recovery effect");
    if extend_recovery_past_absolute {
        fixture
            .db
            .conn
            .execute(
                "UPDATE receiver_jobs SET recovery_expires_at_unix_ms = 2000000
                 WHERE job_id = ?1",
                [fixture.job_id.to_string()],
            )
            .expect("place absolute deadline before recovery expiry");
    }
    let job_id = fixture.job_id;
    drop(fixture);

    let reopened = Db::open_path_with_legacy_identity(
        &path,
        &receiver_workspace_id().to_string(),
        receiver_user_id().as_str(),
    )
    .expect("reopen ownerless due recovery");
    let effect = reopened
        .reconcile_next_receiver_job(now_unix_ms)
        .expect("reconcile ownerless recovery boundary");
    assert_eq!(
        effect.as_ref().map(ReceiverReconciliationEffect::reason),
        expected_reason
    );
    let job = reopened
        .receiver_job(job_id)
        .expect("load ownerless recovery after boundary")
        .expect("ownerless recovery after boundary");
    if let Some(reason) = expected_reason {
        assert_eq!(job.state(), ReceiverJobState::Failed);
        assert_eq!(job.last_error(), Some(reason.as_str()));
        assert!(job.pending_unavailable_notice());
    } else {
        assert_eq!(job.state(), ReceiverJobState::Retrying);
        assert_eq!(job.attempt_kind(), ReceiverAttemptKind::Recovery);
        assert!(!job.pending_unavailable_notice());
    }
}
