use super::receiver_recovery_cleanup::seed_cleanup_pending_recovery;

#[derive(Clone, Copy)]
enum UnprovableCleanup {
    AmbiguousInstance,
    MismatchedSession,
}

#[test]
fn adjacent_cleanup_upgrade_never_releases_ambiguous_or_mismatched_registration() {
    for damage in [
        UnprovableCleanup::AmbiguousInstance,
        UnprovableCleanup::MismatchedSession,
    ] {
        assert_unprovable_cleanup_fails_closed(damage);
    }
}

fn assert_unprovable_cleanup_fails_closed(damage: UnprovableCleanup) {
    let fixture = Fixture::new();
    fixture.seed_pre_receiver_state();
    let first = fixture.run(&["server", "status"]);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let family = fixture.state_db("11111111-1111-4111-8111-111111111111");
    seed_cleanup_pending_recovery(&family);
    let connection = rusqlite::Connection::open(&family).expect("damaged cleanup state");
    connection
        .execute_batch("PRAGMA ignore_check_constraints = ON;")
        .expect("allow partial cleanup tuple");
    let expected_resources = match damage {
        UnprovableCleanup::AmbiguousInstance => {
            connection
                .execute_batch(
                    "UPDATE receiver_jobs SET recovery_cleanup_instance = NULL
                     WHERE job_id = 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb';
                     INSERT INTO brain_sessions
                       (agent_kind, agent_session_id, brain_instance_id, locked_pid,
                        source, workspace_id, actor_id, channel, created_at, last_active_at)
                     VALUES
                       ('claude', 'native-session', 'other-instance', 84, 'fresh',
                        '11111111-1111-4111-8111-111111111111', 'pablo', 'sms', 100, 100);
                     INSERT INTO receiver_session_registrations
                       (workspace_id, conversation_id, agent_kind, actor_id, channel,
                        brain_instance_id, registered_session_id, actual_session_id)
                     VALUES
                       ('11111111-1111-4111-8111-111111111111',
                        'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa', 'claude', 'pablo', 'sms',
                        'other-instance', 'native-session', 'native-session');",
                )
                .expect("make cleanup instance ambiguous");
            (2, 2)
        }
        UnprovableCleanup::MismatchedSession => {
            connection
                .execute_batch(
                    "UPDATE receiver_jobs SET recovery_cleanup_session_id = NULL
                     WHERE job_id = 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb';
                     UPDATE receiver_session_registrations
                     SET actual_session_id = 'other-session'
                     WHERE brain_instance_id = 'ordinary-instance';",
                )
                .expect("mismatch cleanup session registration");
            (1, 1)
        }
    };
    connection
        .execute_batch("PRAGMA ignore_check_constraints = OFF;")
        .expect("restore cleanup constraints");
    drop(connection);
    std::fs::write(
        fixture.xdg_config.join("brain/migrations/version"),
        "0.84.7\n",
    )
    .expect("restore adjacent migration stamp");

    let upgraded = fixture.run(&["server", "status"]);

    assert!(
        upgraded.status.success(),
        "{}",
        String::from_utf8_lossy(&upgraded.stderr)
    );
    let connection = rusqlite::Connection::open(&family).expect("repaired cleanup state");
    let repaired: (String, i64, Option<String>, Option<String>) = connection
        .query_row(
            "SELECT state, pending_unavailable_notice,
                    recovery_cleanup_instance, recovery_cleanup_session_id
             FROM receiver_jobs
             WHERE job_id = 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("load fail-closed cleanup row");
    assert_eq!(repaired, ("failed".to_owned(), 1, None, None));
    let resources: (i64, i64) = connection
        .query_row(
            "SELECT
               (SELECT COUNT(*) FROM receiver_session_registrations
                WHERE conversation_id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa'),
               (SELECT COUNT(*) FROM brain_sessions
                WHERE brain_instance_id IN ('ordinary-instance', 'other-instance')
                  AND locked_pid IS NOT NULL)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("load untouched unprovable resources");
    assert_eq!(resources, expected_resources);
    drop(connection);

    let db = brain::state::Db::open_path_with_legacy_identity(
        &family,
        "11111111-1111-4111-8111-111111111111",
        "pablo",
    )
    .expect("open fail-closed cleanup state");
    assert!(
        db.reconcile_next_receiver_job(201)
            .expect("unprovable cleanup cannot redrive")
            .is_none()
    );
    let job_id = uuid::Uuid::parse_str("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb")
        .expect("job UUID")
        .into();
    let token = brain::state::ReceiverJobToken::parse(
        "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
    )
    .expect("job token");
    assert!(
        !db.acknowledge_receiver_recovery_cleanup(
            job_id,
            token,
            "ordinary-instance",
            "native-session",
            202,
        )
        .expect("unprovable cleanup acknowledgement fails closed")
    );
}
