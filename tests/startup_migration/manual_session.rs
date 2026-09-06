#[test]
fn manual_session_downgrade_preserves_native_history_and_composes_with_receiver_cutover() {
    let fixture = Fixture::new();
    let state = fixture.state_db("11111111-1111-4111-8111-111111111111");
    brain::state::Db::open_path_with_legacy_identity(
        &state,
        "11111111-1111-4111-8111-111111111111",
        "pablo",
    )
    .unwrap();
    let connection = rusqlite::Connection::open(&state).unwrap();
    connection
        .execute_batch(
            "INSERT INTO brain_sessions
               (agent_kind, agent_session_id, brain_instance_id, locked_pid, source,
                workspace_id, actor_id, channel, created_at, last_active_at)
             VALUES ('claude', 'saved-native', 'saved-manual', NULL, 'fresh',
                     '11111111-1111-4111-8111-111111111111', 'pablo', 'interactive', 1, 2);
             INSERT INTO manual_sessions
               (manual_session_id, agent_kind, agent_session_id, workspace_id,
                actor_id, channel, title, position, role)
             VALUES ('saved-manual', 'claude', 'saved-native',
                     '11111111-1111-4111-8111-111111111111', 'pablo', 'interactive',
                     'Brain', 0, 'main');",
        )
        .unwrap();
    let receiver_sql: String = connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE name = 'receiver_jobs'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    for target in ["0.86.15", "0.86.15", "0.85.28"] {
        let output = fixture.run(&[
            "__migrate",
            "--from-version",
            env!("CARGO_PKG_VERSION"),
            "--to-version",
            target,
        ]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, if target == "0.85.28" { 12 } else { 13 });
        let mappings: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name IN
                 ('manual_sessions', 'manual_sessions_one_main')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(mappings, 0);
        let native: (String, String, i64) = connection
            .query_row(
                "SELECT agent_session_id, brain_instance_id, last_active_at
                 FROM brain_sessions WHERE agent_session_id = 'saved-native'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(native, ("saved-native".into(), "saved-manual".into(), 2));
        if target == "0.86.15" {
            let retained: String = connection
                .query_row(
                    "SELECT sql FROM sqlite_master WHERE name = 'receiver_jobs'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(retained, receiver_sql);
        }
    }
}

#[test]
fn manual_session_current_reconciliation_preserves_a_healthy_database_byte_for_byte() {
    let fixture = Fixture::new();
    let state = fixture.state_db("11111111-1111-4111-8111-111111111111");
    brain::state::Db::open_path_with_legacy_identity(
        &state,
        "11111111-1111-4111-8111-111111111111",
        "pablo",
    )
    .unwrap();
    let before = std::fs::read(&state).unwrap();

    let output = fixture.run(&[
        "__migrate",
        "--from-version",
        env!("CARGO_PKG_VERSION"),
        "--to-version",
        env!("CARGO_PKG_VERSION"),
    ]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        std::fs::read(&state).unwrap() == before,
        "current reconciliation rewrote a healthy database"
    );
}

#[test]
fn manual_session_upgrade_and_current_reconciliation_restore_missing_managed_schema() {
    let fixture = Fixture::new();
    let state = fixture.state_db("11111111-1111-4111-8111-111111111111");
    brain::state::Db::open_path_with_legacy_identity(
        &state,
        "11111111-1111-4111-8111-111111111111",
        "pablo",
    )
    .unwrap();
    let connection = rusqlite::Connection::open(&state).unwrap();
    for (from, damage) in [
        (
            "0.86.15",
            "DROP TABLE manual_sessions; PRAGMA user_version = 13;",
        ),
        (
            env!("CARGO_PKG_VERSION"),
            "DROP INDEX manual_sessions_one_main;",
        ),
        (env!("CARGO_PKG_VERSION"), "DROP TABLE manual_sessions;"),
    ] {
        connection.execute_batch(damage).unwrap();
        let output = fixture.run(&[
            "__migrate",
            "--from-version",
            from,
            "--to-version",
            env!("CARGO_PKG_VERSION"),
        ]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let objects: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name IN
                 ('manual_sessions', 'manual_sessions_one_main')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(objects, 2);
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 14);
    }
    assert!(!fixture.state_db("22222222-2222-4222-8222-222222222222").exists());
}
