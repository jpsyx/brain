#[test]
fn second_tui_cannot_claim_recovery_until_exact_cleanup_is_acknowledged() {
    let temporary = tempfile::tempdir().expect("temporary receiver state");
    let path = temporary.path().join("state.db");
    let fixture = accepted_run_in(
        Db::open_path_with_legacy_identity(
            &path,
            &receiver_workspace_id().to_string(),
            receiver_user_id().as_str(),
        )
        .expect("open first TUI state"),
        "two-tui-cleanup-order",
    );
    let second_tui = Db::open_path_with_legacy_identity(
        &path,
        &receiver_workspace_id().to_string(),
        receiver_user_id().as_str(),
    )
    .expect("open second TUI state");

    let effect = fixture
        .db
        .reconcile_next_receiver_job(301_400)
        .expect("first TUI reconciles stale run")
        .expect("first TUI cleanup effect");
    assert_eq!(effect.cleanup_instance(), Some("ordinary-instance"));
    assert_eq!(effect.cleanup_session_id(), Some("native-session"));
    let pending = second_tui
        .receiver_job(fixture.job_id)
        .expect("load cleanup-pending recovery")
        .expect("cleanup-pending recovery");
    assert_eq!(
        pending.recovery_cleanup_instance(),
        Some("ordinary-instance")
    );
    assert_eq!(
        pending.recovery_cleanup_session_id(),
        Some("native-session")
    );
    assert!(
        second_tui
            .claim_next_receiver_recovery_run("second-tui", 301_400, 331_400)
            .expect("second TUI observes cleanup fence")
            .is_none()
    );
    assert!(
        !fixture
            .db
            .acknowledge_receiver_recovery_cleanup(
                fixture.job_id,
                fixture.ordinary.token(),
                "wrong-instance",
                "native-session",
                301_401,
            )
            .expect("reject wrong cleanup acknowledgement")
    );
    assert!(
        second_tui
            .claim_next_receiver_recovery_run("second-tui", 301_401, 331_401)
            .expect("wrong acknowledgement leaves cleanup fence")
            .is_none()
    );
    assert!(
        fixture
            .db
            .acknowledge_receiver_recovery_cleanup(
                fixture.job_id,
                fixture.ordinary.token(),
                "ordinary-instance",
                "native-session",
                301_402,
            )
            .expect("acknowledge exact cleanup")
    );
    let acknowledged = second_tui
        .receiver_job(fixture.job_id)
        .expect("load acknowledged recovery")
        .expect("acknowledged recovery");
    assert_eq!(acknowledged.recovery_cleanup_instance(), None);
    assert_eq!(acknowledged.recovery_cleanup_session_id(), None);
    let registration_count = second_tui
        .conn
        .query_row(
            "SELECT COUNT(*) FROM receiver_session_registrations
             WHERE workspace_id = ?1 AND conversation_id = ?2",
            rusqlite::params![
                receiver_workspace_id().to_string(),
                fixture.ordinary.conversation_id().to_string(),
            ],
            |row| row.get::<_, i64>(0),
        )
        .expect("count acknowledged registrations");
    assert_eq!(registration_count, 0);
    assert_eq!(
        second_tui
            .claim_next_receiver_recovery_run("second-tui", 301_402, 331_402)
            .expect("claim acknowledged recovery")
            .expect("acknowledged recovery claim")
            .job()
            .id(),
        fixture.job_id
    );
}
