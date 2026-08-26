impl Fixture {
    fn state_db(&self, workspace_id: &str) -> PathBuf {
        self.home
            .join(".cache/brain/workspaces")
            .join(workspace_id)
            .join("state.db")
    }

    fn seed_pre_receiver_state(&self) {
        for workspace_id in [
            "11111111-1111-4111-8111-111111111111",
            "22222222-2222-4222-8222-222222222222",
        ] {
            let path = self.state_db(workspace_id);
            std::fs::create_dir_all(path.parent().expect("state parent")).expect("state directory");
            let connection = rusqlite::Connection::open(path).expect("pre-receiver state");
            connection
                .pragma_update(None, "user_version", 5)
                .expect("pre-receiver state version");
        }
    }
}

fn table_exists(path: &Path, table: &str) -> bool {
    let connection = rusqlite::Connection::open(path).expect("state database");
    connection
        .query_row(
            "SELECT EXISTS(
               SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1
             )",
            [table],
            |row| row.get(0),
        )
        .expect("table existence")
}

fn state_schema_version(path: &Path) -> i64 {
    rusqlite::Connection::open(path)
        .expect("state database")
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("state schema version")
}

fn column_exists(path: &Path, table: &str, column: &str) -> bool {
    let connection = rusqlite::Connection::open(path).expect("state database");
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .expect("table columns");
    statement
        .query_map([], |row| row.get::<_, String>(1))
        .expect("query table columns")
        .any(|name| name.expect("column name") == column)
}

fn table_sql(path: &Path, table: &str) -> String {
    rusqlite::Connection::open(path)
        .expect("state database")
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |row| row.get(0),
        )
        .expect("table schema")
}

#[test]
fn receiver_migration_defers_until_legacy_registry_bootstrap_finishes() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.xdg_config.join("brain/env.json"),
        serde_json::to_vec_pretty(&json!({"root": fixture.family}))
            .expect("legacy flat environment"),
    )
    .expect("write legacy flat environment");

    let output = fixture.run(&[
        "workspace",
        "repair",
        "--manifest",
        "--local-user-id",
        "test-user",
    ]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn receiver_migration_reconciles_only_existing_workspace_state_databases() {
    let fixture = Fixture::new();
    let state_paths = [
        fixture.state_db("11111111-1111-4111-8111-111111111111"),
        fixture.state_db("22222222-2222-4222-8222-222222222222"),
    ];
    assert!(state_paths.iter().all(|path| !path.exists()));

    let output = fixture.run(&["server", "status"]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(state_paths.iter().all(|path| !path.exists()));
}

#[test]
fn ordinary_startup_upgrades_and_reconciles_receiver_state_for_every_workspace() {
    let fixture = Fixture::new();
    fixture.seed_pre_receiver_state();

    let first = fixture.run(&["server", "status"]);

    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    for workspace_id in [
        "11111111-1111-4111-8111-111111111111",
        "22222222-2222-4222-8222-222222222222",
    ] {
        let path = fixture.state_db(workspace_id);
        assert!(table_exists(&path, "receiver_conversations"));
        assert!(table_exists(&path, "receiver_jobs"));
        assert!(table_exists(&path, "receiver_session_registrations"));
        assert_eq!(state_schema_version(&path), 10);
    }

    let family = fixture.state_db("11111111-1111-4111-8111-111111111111");
    rusqlite::Connection::open(&family)
        .expect("family state")
        .execute("DROP TABLE receiver_jobs", [])
        .expect("remove managed receiver table");
    let second = fixture.run(&["server", "status"]);

    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(table_exists(&family, "receiver_jobs"));
}

#[test]
fn ordinary_startup_repairs_a_missing_launch_retry_column_in_damaged_v7_schema() {
    let fixture = Fixture::new();
    fixture.seed_pre_receiver_state();
    let first = fixture.run(&["server", "status"]);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let family = fixture.state_db("11111111-1111-4111-8111-111111111111");
    let connection = rusqlite::Connection::open(&family).expect("family state");
    connection
        .execute_batch(
            "DROP TABLE receiver_session_registrations;
             ALTER TABLE receiver_jobs DROP COLUMN retry_from_state;",
        )
        .expect("restore a damaged v7 receiver schema");
    connection
        .pragma_update(None, "user_version", 7)
        .expect("restore damaged v7 schema version");
    drop(connection);
    assert_eq!(state_schema_version(&family), 7);
    assert!(!table_exists(&family, "receiver_session_registrations"));

    let second = fixture.run(&["server", "status"]);

    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(column_exists(
        &family,
        "receiver_jobs",
        "retry_from_state"
    ));
    assert!(table_exists(&family, "receiver_session_registrations"));
    assert_eq!(state_schema_version(&family), 10);
}

#[test]
fn explicit_down_migration_removes_only_receiver_recovery_state() {
    let fixture = Fixture::new();
    fixture.seed_pre_receiver_state();
    let up = fixture.run(&["server", "status"]);
    assert!(
        up.status.success(),
        "{}",
        String::from_utf8_lossy(&up.stderr)
    );

    let down = fixture.run(&[
        "__migrate",
        "--from-version",
        env!("CARGO_PKG_VERSION"),
        "--to-version",
        "0.83.9",
    ]);

    assert!(
        down.status.success(),
        "{}",
        String::from_utf8_lossy(&down.stderr)
    );
    for workspace_id in [
        "11111111-1111-4111-8111-111111111111",
        "22222222-2222-4222-8222-222222222222",
    ] {
        let path = fixture.state_db(workspace_id);
        assert!(!column_exists(&path, "receiver_jobs", "attempt_kind"));
        assert!(!column_exists(
            &path,
            "receiver_jobs",
            "launch_expires_at_unix_ms"
        ));
        assert!(column_exists(
            &path,
            "receiver_jobs",
            "observation_revision"
        ));
        assert_eq!(state_schema_version(&path), 9);
    }
}

#[test]
fn explicit_down_migration_removes_only_receiver_session_registration_state() {
    let fixture = Fixture::new();
    fixture.seed_pre_receiver_state();
    let up = fixture.run(&["server", "status"]);
    assert!(
        up.status.success(),
        "{}",
        String::from_utf8_lossy(&up.stderr)
    );

    let down = fixture.run(&[
        "__migrate",
        "--from-version",
        env!("CARGO_PKG_VERSION"),
        "--to-version",
        "0.75.0",
    ]);

    assert!(
        down.status.success(),
        "{}",
        String::from_utf8_lossy(&down.stderr)
    );
    for workspace_id in [
        "11111111-1111-4111-8111-111111111111",
        "22222222-2222-4222-8222-222222222222",
    ] {
        let path = fixture.state_db(workspace_id);
        assert!(!table_exists(&path, "receiver_session_registrations"));
        assert!(column_exists(&path, "receiver_jobs", "retry_from_state"));
        assert_eq!(state_schema_version(&path), 7);
    }
}

#[test]
fn explicit_down_migration_removes_only_the_receiver_launch_retry_origin() {
    let fixture = Fixture::new();
    fixture.seed_pre_receiver_state();
    let up = fixture.run(&["server", "status"]);
    assert!(
        up.status.success(),
        "{}",
        String::from_utf8_lossy(&up.stderr)
    );

    let down = fixture.run(&[
        "__migrate",
        "--from-version",
        env!("CARGO_PKG_VERSION"),
        "--to-version",
        "0.74.4",
    ]);

    assert!(
        down.status.success(),
        "{}",
        String::from_utf8_lossy(&down.stderr)
    );
    for workspace_id in [
        "11111111-1111-4111-8111-111111111111",
        "22222222-2222-4222-8222-222222222222",
    ] {
        let path = fixture.state_db(workspace_id);
        assert!(table_exists(&path, "receiver_jobs"));
        assert!(!column_exists(&path, "receiver_jobs", "retry_from_state"));
        assert_eq!(state_schema_version(&path), 6);
    }
}

#[test]
fn observation_down_rebuilds_a_v8_compatible_table_before_the_remaining_down_chain() {
    let fixture = Fixture::new();
    fixture.seed_pre_receiver_state();
    let up = fixture.run(&["server", "status"]);
    assert!(
        up.status.success(),
        "{}",
        String::from_utf8_lossy(&up.stderr)
    );

    let down = fixture.run(&[
        "__migrate",
        "--from-version",
        env!("CARGO_PKG_VERSION"),
        "--to-version",
        "0.74.4",
    ]);

    assert!(
        down.status.success(),
        "{}",
        String::from_utf8_lossy(&down.stderr)
    );
    for workspace_id in [
        "11111111-1111-4111-8111-111111111111",
        "22222222-2222-4222-8222-222222222222",
    ] {
        let path = fixture.state_db(workspace_id);
        let receiver_jobs = table_sql(&path, "receiver_jobs");
        assert!(!receiver_jobs.contains("job_token"));
        assert!(!receiver_jobs.contains("'launched'"));
        assert!(!column_exists(&path, "receiver_jobs", "retry_from_state"));
        assert_eq!(state_schema_version(&path), 6);
    }
}

#[test]
fn explicit_down_migration_removes_receiver_schema_from_every_workspace() {
    let fixture = Fixture::new();
    fixture.seed_pre_receiver_state();
    let up = fixture.run(&["server", "status"]);
    assert!(
        up.status.success(),
        "{}",
        String::from_utf8_lossy(&up.stderr)
    );

    let down = fixture.run(&[
        "__migrate",
        "--from-version",
        env!("CARGO_PKG_VERSION"),
        "--to-version",
        "0.71.38",
    ]);

    assert!(
        down.status.success(),
        "{}",
        String::from_utf8_lossy(&down.stderr)
    );
    for workspace_id in [
        "11111111-1111-4111-8111-111111111111",
        "22222222-2222-4222-8222-222222222222",
    ] {
        let path = fixture.state_db(workspace_id);
        assert!(!table_exists(&path, "receiver_jobs"));
        assert!(!table_exists(&path, "receiver_conversations"));
        assert_eq!(state_schema_version(&path), 5);
    }
}
