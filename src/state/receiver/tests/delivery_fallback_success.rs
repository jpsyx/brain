struct AcknowledgedFallbackFixture {
    db: Db,
    job_id: ReceiverJobId,
}

fn acknowledged_fallback_fixture(db: Db) -> AcknowledgedFallbackFixture {
    let fixture = super::binding::completion_fixture_in(db, ReceiverJobState::Processing);
    fixture
        .db
        .complete_receiver_job_with_binding(&fixture.request())
        .expect("record durable answer")
        .expect("exact completion owner");
    assert!(
        fixture
            .db
            .acknowledge_receiver_answer_controller_shutdown(
                fixture.job_id,
                fixture.token,
                fixture.registration.instance(),
                42,
                1_600,
            )
            .expect("acknowledge confirmed controller exit")
    );
    fixture
        .db
        .conn
        .execute(
            "UPDATE receiver_deliveries SET frozen_fallbacks_json = ?2 WHERE job_id = ?1",
            rusqlite::params![
                fixture.job_id.to_string(),
                serde_json::json!([{
                    "provider": "resend",
                    "sender": "brain@example.test",
                    "recipient": "safe@example.test"
                }])
                .to_string(),
            ],
        )
        .expect("freeze authenticated fallback");
    let primary_claim = fixture
        .db
        .claim_next_receiver_delivery("primary-owner", 2_000, 32_000)
        .expect("claim primary response")
        .expect("primary response");
    assert!(
        fixture
            .db
            .mark_receiver_delivery_io_started(&primary_claim, 2_100)
            .expect("mark primary provider IO")
    );
    assert_eq!(
        fixture
            .db
            .apply_receiver_delivery_result(
                &primary_claim,
                2_200,
                ReceiverProviderResultClass::PermanentlyRejected(
                    ReceiverDeliveryErrorCategory::ProviderRejected,
                ),
            )
            .expect("terminalize primary response"),
        ReceiverDeliveryApplyOutcome::Applied
    );
    let fallback_claim = fixture
        .db
        .claim_next_receiver_delivery("fallback-owner", 2_300, 32_300)
        .expect("claim fallback notice")
        .expect("fallback notice");
    assert!(
        fixture
            .db
            .mark_receiver_delivery_io_started(&fallback_claim, 2_400)
            .expect("mark fallback provider IO")
    );
    let reference = ReceiverProviderReference::parse("fallback-provider-reference")
        .expect("provider reference");
    assert!(
        fixture
            .db
            .apply_receiver_delivery_result(
                &fallback_claim,
                2_500,
                ReceiverProviderResultClass::Acknowledged(reference.clone()),
            )
            .expect("acknowledge fallback notice")
            == ReceiverDeliveryApplyOutcome::Applied
    );
    assert!(
        fixture
            .db
            .apply_receiver_delivery_result(
                &fallback_claim,
                2_501,
                ReceiverProviderResultClass::Acknowledged(reference),
            )
            .expect("ignore duplicate fallback acknowledgement")
            == ReceiverDeliveryApplyOutcome::Stale
    );
    AcknowledgedFallbackFixture {
        db: fixture.db,
        job_id: fixture.job_id,
    }
}

fn assert_acknowledged_fallback_audit(db: &Db, job_id: ReceiverJobId) {
    let rows: Vec<(String, String, Option<String>, bool)> = {
        let mut statement = db
            .conn
            .prepare(
                "SELECT response_kind, state, fallback_decision,
                        provider_reference IS NOT NULL
                          AND length(trim(provider_reference)) > 0
                 FROM receiver_deliveries WHERE job_id = ?1
                 ORDER BY response_kind",
            )
            .expect("prepare fallback audit rows");
        statement
            .query_map([job_id.to_string()], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .expect("query fallback audit rows")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("collect fallback audit rows")
    };
    assert_eq!(rows.len(), 2);
    assert_eq!(
        (
            rows[0].0.as_str(),
            rows[0].1.as_str(),
            rows[0].2.as_deref(),
            rows[0].3,
        ),
        (
            "fallback-notice",
            "acknowledged",
            None,
            true,
        )
    );
    assert_eq!(
        (
            rows[1].0.as_str(),
            rows[1].1.as_str(),
            rows[1].2.as_deref(),
            rows[1].3,
        ),
        (
            "final-answer",
            "failed",
            Some("fallback-planned"),
            false,
        )
    );
    assert_eq!(
        db.receiver_job(job_id)
            .expect("load fallback job")
            .expect("fallback job remains")
            .state(),
        ReceiverJobState::Done
    );
}

#[test]
fn acknowledged_fallback_remains_done_across_reopen_and_repeated_repair() {
    let temporary = tempfile::tempdir().expect("temporary state directory");
    let path = std::fs::canonicalize(temporary.path())
        .expect("canonical temporary state directory")
        .join("state.db");
    let workspace = super::support::receiver_workspace_id().to_string();
    let actor = super::support::receiver_user_id();
    let fixture = acknowledged_fallback_fixture(
        Db::open_path_with_legacy_identity(&path, &workspace, actor.as_str())
            .expect("file-backed receiver state"),
    );
    let job_id = fixture.job_id;
    assert_acknowledged_fallback_audit(&fixture.db, job_id);
    drop(fixture);

    let reopened = Db::open_path_with_legacy_identity(&path, &workspace, actor.as_str())
        .expect("reopen acknowledged fallback state");

    assert_acknowledged_fallback_audit(&reopened, job_id);
    super::super::schema::up(&reopened.conn, 12).expect("first repeated repair");
    super::super::schema::up(&reopened.conn, 12).expect("second repeated repair");
    assert_acknowledged_fallback_audit(&reopened, job_id);
    assert!(
        reopened
            .claim_next_receiver_delivery("post-repair-owner", 2_600, 32_600)
            .expect("probe post-repair delivery")
            .is_none(),
        "repair recreated fallback resend authority"
    );
}

#[test]
fn acknowledged_fallback_remains_done_across_v12_down_v11_reopen_and_reupgrade() {
    let temporary = tempfile::tempdir().expect("temporary state directory");
    let path = std::fs::canonicalize(temporary.path())
        .expect("canonical temporary state directory")
        .join("state.db");
    let workspace = super::support::receiver_workspace_id().to_string();
    let actor = super::support::receiver_user_id();
    let fixture = acknowledged_fallback_fixture(
        Db::open_path_with_legacy_identity(&path, &workspace, actor.as_str())
            .expect("file-backed receiver state"),
    );
    let job_id = fixture.job_id;
    assert_acknowledged_fallback_audit(&fixture.db, job_id);
    drop(fixture);

    super::super::schema::down_delivery_path(&path).expect("downgrade acknowledged fallback");
    let downgraded = rusqlite::Connection::open(&path).expect("reopen v11 state");
    let version: i64 = downgraded
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("v11 schema version");
    let v11_state: (String, Option<String>) = downgraded
        .query_row(
            "SELECT state, last_error FROM receiver_jobs WHERE job_id = ?1",
            [job_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("load downgraded fallback job");
    let delivery_table_exists: bool = downgraded
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master
             WHERE type = 'table' AND name = 'receiver_deliveries')",
            [],
            |row| row.get(0),
        )
        .expect("inspect downgraded delivery table");
    assert_eq!(version, 11);
    assert_eq!(v11_state, ("done".to_owned(), None));
    assert!(!delivery_table_exists);
    drop(downgraded);

    let upgraded = Db::open_path_with_legacy_identity(&path, &workspace, actor.as_str())
        .expect("re-upgrade acknowledged fallback state");
    assert_eq!(
        upgraded
            .receiver_job(job_id)
            .expect("load upgraded fallback job")
            .expect("upgraded fallback job remains")
            .state(),
        ReceiverJobState::Done
    );
    let delivery_count: i64 = upgraded
        .conn
        .query_row(
            "SELECT COUNT(*) FROM receiver_deliveries WHERE job_id = ?1",
            [job_id.to_string()],
            |row| row.get(0),
        )
        .expect("count re-upgraded delivery rows");
    assert_eq!(delivery_count, 0);
    assert!(
        upgraded
            .claim_next_receiver_delivery("post-upgrade-owner", 2_600, 32_600)
            .expect("probe post-upgrade delivery")
            .is_none(),
        "re-upgrade recreated fallback resend authority"
    );
}
