fn stored_agent_kind_check(connection: &rusqlite::Connection, table: &str) -> String {
    let sql: String = connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |row| row.get(0),
        )
        .unwrap_or_else(|error| panic!("stored {table} SQL: {error}"));
    sql.lines()
        .find(|line| line.contains("agent_kind") && line.contains("CHECK"))
        .unwrap_or_else(|| panic!("{table} has no agent_kind CHECK"))
        .trim()
        .to_owned()
}

const PI_TABLES: [&str; 3] = [
    "manual_sessions",
    "receiver_session_registrations",
    "receiver_answer_cleanups",
];

/// A database created before Brain learned pi keeps its old `CHECK`, and
/// `CREATE TABLE IF NOT EXISTS` never repairs one, so opening it has to.
#[test]
fn opening_a_pre_pi_database_widens_every_stored_frontend_contract() {
    let fixture = Fixture::new();
    let state = fixture.state_db("11111111-1111-4111-8111-111111111111");
    brain::state::Db::open_path_with_legacy_identity(
        &state,
        "11111111-1111-4111-8111-111111111111",
        "pablo",
    )
    .unwrap();

    let output = fixture.run(&[
        "__migrate",
        "--from-version",
        env!("CARGO_PKG_VERSION"),
        "--to-version",
        "0.90.1",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let connection = rusqlite::Connection::open(&state).unwrap();
    for table in PI_TABLES {
        assert!(
            !stored_agent_kind_check(&connection, table).contains("'pi'"),
            "{table} kept pi after the downgrade"
        );
    }
    drop(connection);

    let output = fixture.run(&[
        "__migrate",
        "--from-version",
        "0.90.1",
        "--to-version",
        env!("CARGO_PKG_VERSION"),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let connection = rusqlite::Connection::open(&state).unwrap();
    for table in PI_TABLES {
        assert!(
            stored_agent_kind_check(&connection, table).contains("'pi'"),
            "{table} still rejects pi after the upgrade"
        );
    }
    connection
        .execute_batch(
            "INSERT INTO brain_sessions
               (agent_kind, agent_session_id, brain_instance_id, locked_pid, source,
                workspace_id, actor_id, channel, created_at, last_active_at)
             VALUES ('pi', 'pi-native', 'pi-manual', NULL, 'fresh',
                     '11111111-1111-4111-8111-111111111111', 'pablo', 'interactive', 1, 2);
             INSERT INTO manual_sessions
               (manual_session_id, agent_kind, agent_session_id, workspace_id,
                actor_id, channel, title, position, role)
             VALUES ('pi-manual', 'pi', 'pi-native',
                     '11111111-1111-4111-8111-111111111111', 'pablo', 'interactive',
                     'Brain', 0, 'main');",
        )
        .expect("a widened contract accepts a pi row");
}

/// A downgrade runs from this binary, so it must leave a database the previous
/// Brain accepts: the old contract, and none of the rows only pi could own.
#[test]
fn downgrading_drops_pi_rows_and_restores_the_previous_contract() {
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
             VALUES ('pi', 'pi-native', 'pi-manual', NULL, 'fresh',
                     '11111111-1111-4111-8111-111111111111', 'pablo', 'interactive', 1, 2),
                    ('claude', 'claude-native', 'claude-manual', NULL, 'fresh',
                     '11111111-1111-4111-8111-111111111111', 'pablo', 'interactive', 1, 2);
             INSERT INTO manual_sessions
               (manual_session_id, agent_kind, agent_session_id, workspace_id,
                actor_id, channel, title, position, role)
             VALUES ('pi-manual', 'pi', 'pi-native',
                     '11111111-1111-4111-8111-111111111111', 'pablo', 'interactive',
                     'Brain', 0, 'main'),
                    ('claude-manual', 'claude', 'claude-native',
                     '11111111-1111-4111-8111-111111111111', 'pablo', 'interactive',
                     'Brain', 0, 'main');",
        )
        .unwrap();
    drop(connection);

    let output = fixture.run(&[
        "__migrate",
        "--from-version",
        env!("CARGO_PKG_VERSION"),
        "--to-version",
        "0.90.1",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let connection = rusqlite::Connection::open(&state).unwrap();
    let kinds: Vec<String> = connection
        .prepare("SELECT agent_kind FROM manual_sessions ORDER BY agent_kind")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(kinds, vec!["claude".to_owned()]);
    for table in PI_TABLES {
        assert!(
            !stored_agent_kind_check(&connection, table).contains("'pi'"),
            "{table} kept the pi contract after the downgrade"
        );
    }
    // The index the rebuild drops with the table has to come back, or the
    // previous Brain loses its one-main guarantee.
    let index: i64 = connection
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name = 'manual_sessions_one_main'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(index, 1);
}
