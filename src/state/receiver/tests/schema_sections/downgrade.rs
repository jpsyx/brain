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
