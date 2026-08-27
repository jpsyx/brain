#[test]
fn v9_down_maps_every_ambiguous_or_postspawn_lifecycle_to_old_nonclaimable_state() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = temporary.path().join("state.db");
    let states = [
        "queued",
        "claimed",
        "launching",
        "launched",
        "accepted",
        "processing",
        "answer-ready",
        "delivering",
        "retrying",
        "failed",
        "done",
    ];
    let jobs = {
        let db = Db::open_path_with_legacy_identity(
            &path,
            &receiver_workspace_id().to_string(),
            receiver_user_id().as_str(),
        )
        .expect("receiver state");
        let identity =
            ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
        states
            .iter()
            .enumerate()
            .map(|(index, state)| {
                let accepted = db
                    .accept_receiver_job(
                        &receiver_job(Some(&format!("down-state-{index}")), 100 + index as u64),
                        &identity,
                    )
                    .expect("accept receiver job");
                let leased = !matches!(*state, "queued" | "failed" | "done");
                db.conn
                    .execute(
                        "UPDATE receiver_jobs
                         SET state = ?1, claim_owner = ?2, claim_expires_at_unix_ms = ?3,
                             retry_at_unix_ms = ?4, retry_from_state = ?5
                         WHERE job_id = ?6",
                        rusqlite::params![
                            state,
                            leased.then_some("old-owner"),
                            leased.then_some(100_i64),
                            (*state == "retrying").then_some(100_i64),
                            (*state == "retrying").then_some("delivering"),
                            accepted.job_id().to_string(),
                        ],
                    )
                    .expect("seed v9 lifecycle state");
                (accepted.job_id().to_string(), *state)
            })
            .collect::<Vec<_>>()
    };

    super::super::schema::down_unavailable_notice_path(&path)
        .expect("downgrade receiver notice lease");
    super::super::schema::down_recovery_to_observation_path(&path)
        .expect("downgrade receiver recovery metadata");
    super::super::schema::down_observation_to_registration_path(&path)
        .expect("downgrade receiver observations");

    let connection = rusqlite::Connection::open(path).expect("downgraded state");
    for (job_id, original) in jobs {
        let (downgraded, old_claimable): (String, bool) = connection
            .query_row(
                "SELECT state,
                        state = 'queued'
                        OR (state = 'retrying' AND retry_at_unix_ms <= 100)
                        OR (state NOT IN ('failed', 'done')
                            AND claim_owner IS NOT NULL
                            AND claim_expires_at_unix_ms <= 100)
                 FROM receiver_jobs WHERE job_id = ?1",
                [job_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("load downgraded lifecycle");
        let postspawn_or_ambiguous = matches!(
            original,
            "launching"
                | "launched"
                | "accepted"
                | "processing"
                | "answer-ready"
                | "delivering"
                | "retrying"
        );
        assert_eq!(
            downgraded,
            if postspawn_or_ambiguous {
                "failed"
            } else {
                original
            },
            "unsafe v8 representation for {original}"
        );
        assert_eq!(
            old_claimable,
            matches!(original, "queued" | "claimed"),
            "old coordinator claim result for {original}"
        );
    }
}

#[test]
fn v10_down_preserves_ordinary_v9_work_but_terminalizes_recovery_attempts() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = temporary.path().join("state.db");
    let (ordinary_id, recovery_id, recovery_token, recovery_inbound) = {
        let db = Db::open_path_with_legacy_identity(
            &path,
            &receiver_workspace_id().to_string(),
            receiver_user_id().as_str(),
        )
        .expect("receiver state");
        let identity =
            ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
        let ordinary = db
            .accept_receiver_job(&receiver_job(Some("v10-down-ordinary"), 100), &identity)
            .expect("accept ordinary job");
        let recovery = db
            .accept_receiver_job(&receiver_job(Some("v10-down-recovery"), 200), &identity)
            .expect("accept recovery job");
        db.conn
            .execute(
                "UPDATE receiver_jobs
                 SET state = 'launched', claim_owner = 'ordinary-owner',
                     claim_expires_at_unix_ms = 1_100, launch_expires_at_unix_ms = 1_200,
                     acceptance_expires_at_unix_ms = 1_300
                 WHERE job_id = ?1",
                [ordinary.job_id().to_string()],
            )
            .expect("seed ordinary launch");
        db.conn
            .execute(
                "UPDATE receiver_jobs
                 SET state = 'retrying', retry_at_unix_ms = 1_000,
                     retry_from_state = 'accepted', recovery_count = 1,
                     attempt_kind = 'recovery', recovery_expires_at_unix_ms = 2_000,
                     accepted_at_unix_ms = 900, observation_revision = 0
                 WHERE job_id = ?1",
                [recovery.job_id().to_string()],
            )
            .expect("seed recovery attempt");
        let (token, inbound): (String, Vec<u8>) = db
            .conn
            .query_row(
                "SELECT job_token, CAST(inbound_json AS BLOB)
                 FROM receiver_jobs WHERE job_id = ?1",
                [recovery.job_id().to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("load recovery identity");
        (
            ordinary.job_id().to_string(),
            recovery.job_id().to_string(),
            token,
            inbound,
        )
    };

    super::super::schema::down_unavailable_notice_path(&path)
        .expect("downgrade receiver notice lease");
    super::super::schema::down_recovery_to_observation_path(&path)
        .expect("downgrade receiver recovery schema");

    let connection = rusqlite::Connection::open(path).expect("downgraded state");
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("schema version");
    assert_eq!(version, 9);
    let recovery_columns: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('receiver_jobs')
             WHERE name IN (
               'attempt_kind', 'recovery_count', 'launch_expires_at_unix_ms',
               'pending_unavailable_notice', 'recovery_cleanup_instance',
               'recovery_cleanup_session_id'
             )",
            [],
            |row| row.get(0),
        )
        .expect("count recovery columns");
    assert_eq!(recovery_columns, 0);
    let ordinary_state: String = connection
        .query_row(
            "SELECT state FROM receiver_jobs WHERE job_id = ?1",
            [ordinary_id],
            |row| row.get(0),
        )
        .expect("load ordinary state");
    assert_eq!(ordinary_state, "launched");
    let recovered: (String, Option<String>, Option<i64>, Option<String>, String, Vec<u8>) =
        connection
            .query_row(
                "SELECT state, claim_owner, retry_at_unix_ms, retry_from_state,
                        job_token, CAST(inbound_json AS BLOB)
                 FROM receiver_jobs WHERE job_id = ?1",
                [recovery_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .expect("load downgraded recovery");
    assert_eq!(recovered.0, "failed");
    assert_eq!(recovered.1, None);
    assert_eq!(recovered.2, None);
    assert_eq!(recovered.3, None);
    assert_eq!(recovered.4, recovery_token);
    assert_eq!(recovered.5, recovery_inbound);
}
