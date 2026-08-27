pub(super) fn seed_cleanup_pending_recovery(path: &Path) {
    rusqlite::Connection::open(path)
        .expect("cleanup-fence state")
        .execute_batch(
            r#"CREATE TABLE IF NOT EXISTS brain_sessions (
               agent_kind        TEXT NOT NULL,
               agent_session_id  TEXT NOT NULL,
               brain_instance_id TEXT NOT NULL,
               locked_pid        INTEGER,
               source            TEXT,
               workspace_id      TEXT NOT NULL,
               actor_id          TEXT NOT NULL,
               channel           TEXT NOT NULL,
               created_at        INTEGER NOT NULL,
               last_active_at    INTEGER NOT NULL,
               PRIMARY KEY
                 (agent_kind, agent_session_id, workspace_id, actor_id, channel)
             );
             INSERT INTO receiver_conversations
               (conversation_id, workspace_id, user_id, channel, conversation_key,
                agent_kind, agent_session_id, created_at_unix_ms, updated_at_unix_ms)
             VALUES
               ('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa',
                '11111111-1111-4111-8111-111111111111', 'pablo', 'sms',
                'cleanup-fence-conversation', 'codex', 'native-session', 100, 100);
             INSERT INTO brain_sessions
               (agent_kind, agent_session_id, brain_instance_id, locked_pid, source,
                workspace_id, actor_id, channel, created_at, last_active_at)
             VALUES
               ('codex', 'native-session', 'ordinary-instance', 42, 'fresh',
                '11111111-1111-4111-8111-111111111111', 'pablo', 'sms', 100, 100);
             INSERT INTO receiver_session_registrations
               (workspace_id, conversation_id, agent_kind, actor_id, channel,
                brain_instance_id, registered_session_id, actual_session_id)
             VALUES
               ('11111111-1111-4111-8111-111111111111',
                'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa', 'codex', 'pablo', 'sms',
                'ordinary-instance', 'native-session', 'native-session');
             INSERT INTO receiver_jobs
               (job_id, job_token, workspace_id, conversation_id, channel, inbound_json,
                state, received_at_unix_ms, updated_at_unix_ms, retry_count,
                retry_at_unix_ms, retry_from_state, last_error,
                recovery_expires_at_unix_ms, absolute_work_expires_at_unix_ms,
                recovery_count, attempt_kind, pending_unavailable_notice,
                recovery_cleanup_instance, recovery_cleanup_session_id)
             VALUES
               ('bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb',
                'cccccccc-cccc-4ccc-8ccc-cccccccccccc',
                '11111111-1111-4111-8111-111111111111',
                'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa', 'sms',
                '{"job_id":"bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
                  "workspace_id":"11111111-1111-4111-8111-111111111111",
                  "actor":{"user_id":"pablo","display_name":"Pablo","channel":"sms"},
                  "channel":"sms","authenticated_sender":"sender","prompt":"",
                  "attachments":[],"received_at_unix_ms":100}', 'retrying',
                100, 200, 0, 200, 'processing', 'recovery-accepted-stall',
                600000, 1800000, 1, 'recovery', 0,
                'ordinary-instance', 'native-session');"#,
        )
        .expect("seed cleanup-pending recovery");
}

struct CleanupState {
    state: String,
    owner: Option<String>,
    retry_at: Option<i64>,
    error: Option<String>,
    pending_notice: i64,
    instance: Option<String>,
    session: Option<String>,
}

#[test]
fn adjacent_cleanup_fence_down_migration_terminalizes_without_losing_exact_cleanup() {
    let fixture = Fixture::new();
    fixture.seed_pre_receiver_state();
    let up = fixture.run(&["server", "status"]);
    assert!(
        up.status.success(),
        "{}",
        String::from_utf8_lossy(&up.stderr)
    );
    let family = fixture.state_db("11111111-1111-4111-8111-111111111111");
    seed_cleanup_pending_recovery(&family);

    let down = fixture.run(&[
        "__migrate",
        "--from-version",
        env!("CARGO_PKG_VERSION"),
        "--to-version",
        "0.84.7",
    ]);

    assert!(
        down.status.success(),
        "{}",
        String::from_utf8_lossy(&down.stderr)
    );
    let connection = rusqlite::Connection::open(&family).expect("downgraded cleanup state");
    let row = connection
        .query_row(
            "SELECT state, claim_owner, retry_at_unix_ms, last_error,
                    pending_unavailable_notice, recovery_cleanup_instance,
                    recovery_cleanup_session_id
             FROM receiver_jobs
             WHERE job_id = 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb'",
            [],
            |row| {
                Ok(CleanupState {
                    state: row.get(0)?,
                    owner: row.get(1)?,
                    retry_at: row.get(2)?,
                    error: row.get(3)?,
                    pending_notice: row.get(4)?,
                    instance: row.get(5)?,
                    session: row.get(6)?,
                })
            },
        )
        .expect("load downgraded cleanup recovery");
    assert_eq!(row.state, "failed");
    assert_eq!(row.owner, None);
    assert_eq!(row.retry_at, None);
    assert_eq!(
        row.error.as_deref(),
        Some("recovery-native-session-unavailable")
    );
    assert_eq!(row.pending_notice, 1);
    assert_eq!(row.instance.as_deref(), Some("ordinary-instance"));
    assert_eq!(row.session.as_deref(), Some("native-session"));
    let registration_and_lock: (i64, Option<i64>) = connection
        .query_row(
            "SELECT
               (SELECT COUNT(*) FROM receiver_session_registrations
                WHERE brain_instance_id = 'ordinary-instance'),
               (SELECT locked_pid FROM brain_sessions
                WHERE brain_instance_id = 'ordinary-instance')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("load retained exact cleanup registration");
    assert_eq!(registration_and_lock, (1, Some(42)));
    assert_eq!(
        connection
            .execute(
                "UPDATE receiver_jobs
                 SET state = 'claimed', claim_owner = 'old-owner',
                     claim_expires_at_unix_ms = 1000
                 WHERE job_id = 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb'
                   AND state = 'retrying' AND attempt_kind = 'recovery'",
                [],
            )
            .expect("simulate unfenced old recovery claim"),
        0
    );
}

#[derive(Clone, Copy)]
enum MissingCleanupHalf {
    Instance,
    Session,
}

#[test]
fn adjacent_cleanup_fence_upgrade_reconstructs_missing_instance() {
    assert_adjacent_cleanup_fence_upgrade(MissingCleanupHalf::Instance);
}

#[test]
fn adjacent_cleanup_fence_upgrade_reconstructs_missing_session() {
    assert_adjacent_cleanup_fence_upgrade(MissingCleanupHalf::Session);
}

fn assert_adjacent_cleanup_fence_upgrade(missing: MissingCleanupHalf) {
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
    let damage_sql = match missing {
        MissingCleanupHalf::Instance => {
            "UPDATE receiver_jobs SET recovery_cleanup_instance = NULL
             WHERE job_id = 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb'"
        }
        MissingCleanupHalf::Session => {
            "UPDATE receiver_jobs SET recovery_cleanup_session_id = NULL
             WHERE job_id = 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb'"
        }
    };
    connection.execute(damage_sql, []).expect("damage cleanup tuple");
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
    let repaired: (String, Option<String>, i64, Option<String>, Option<String>) = connection
        .query_row(
            "SELECT state, last_error, pending_unavailable_notice,
                    recovery_cleanup_instance, recovery_cleanup_session_id
             FROM receiver_jobs
             WHERE job_id = 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .expect("load repaired cleanup row");
    assert_eq!(repaired.0, "failed");
    assert_eq!(
        repaired.1.as_deref(),
        Some("recovery-native-session-unavailable")
    );
    assert_eq!(repaired.2, 1);
    assert_eq!(repaired.3.as_deref(), Some("ordinary-instance"));
    assert_eq!(repaired.4.as_deref(), Some("native-session"));
    drop(connection);
    assert_cleanup_resources(&family, 1, Some(42));

    let db = brain::state::Db::open_path_with_legacy_identity(
        &family,
        "11111111-1111-4111-8111-111111111111",
        "pablo",
    )
    .expect("open repaired cleanup state");
    let effect = db
        .reconcile_next_receiver_job(201)
        .expect("redrive reconstructed cleanup")
        .expect("reconstructed cleanup effect");
    assert_eq!(
        effect.action(),
        brain::state::ReceiverReconciliationAction::TerminalFailure
    );
    assert_eq!(
        effect.reason(),
        brain::state::ReceiverReconciliationReason::NativeSessionUnavailable
    );
    assert_eq!(effect.cleanup_instance(), Some("ordinary-instance"));
    assert_eq!(effect.cleanup_session_id(), Some("native-session"));
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
            "wrong-instance",
            "native-session",
            202,
        )
        .expect("reject wrong reconstructed cleanup acknowledgement")
    );
    assert_cleanup_resources(&family, 1, Some(42));
    assert!(
        db.acknowledge_receiver_recovery_cleanup(
            job_id,
            token,
            "ordinary-instance",
            "native-session",
            203,
        )
        .expect("acknowledge reconstructed cleanup")
    );
    assert_cleanup_resources(&family, 0, None);
    assert!(
        db.reconcile_next_receiver_job(204)
            .expect("cleanup stops redriving after acknowledgement")
            .is_none()
    );
}

fn assert_cleanup_resources(path: &Path, expected_registrations: i64, expected_lock: Option<i64>) {
    let connection = rusqlite::Connection::open(path).expect("cleanup resource state");
    let resources: (i64, Option<i64>) = connection
        .query_row(
            "SELECT
               (SELECT COUNT(*) FROM receiver_session_registrations
                WHERE workspace_id = '11111111-1111-4111-8111-111111111111'
                  AND conversation_id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa'
                  AND brain_instance_id = 'ordinary-instance'),
               (SELECT locked_pid FROM brain_sessions
                WHERE workspace_id = '11111111-1111-4111-8111-111111111111'
                  AND brain_instance_id = 'ordinary-instance'
                  AND agent_session_id = 'native-session')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("load cleanup resources");
    assert_eq!(resources, (expected_registrations, expected_lock));
}
