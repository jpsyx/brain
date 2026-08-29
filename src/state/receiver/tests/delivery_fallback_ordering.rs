#[derive(Clone, Copy, Debug)]
enum RestoredFallbackSourceKind {
    FinalAnswer,
    ControlAcknowledgement,
    UnavailableNotice,
}

impl RestoredFallbackSourceKind {
    const ALL: [Self; 3] = [
        Self::FinalAnswer,
        Self::ControlAcknowledgement,
        Self::UnavailableNotice,
    ];

    const fn response_kind(self) -> ReceiverResponseKind {
        match self {
            Self::FinalAnswer => ReceiverResponseKind::FinalAnswer,
            Self::ControlAcknowledgement => ReceiverResponseKind::ControlAcknowledgement,
            Self::UnavailableNotice => ReceiverResponseKind::UnavailableNotice,
        }
    }
}

struct ReversedFallbackFixture {
    db: Db,
    job_id: ReceiverJobId,
}

fn reversed_fallback_fixture(
    path: &std::path::Path,
    workspace: &str,
    actor: &str,
    source_kind: RestoredFallbackSourceKind,
) -> ReversedFallbackFixture {
    let db = Db::open_path_with_legacy_identity(path, workspace, actor)
        .expect("file-backed receiver state");
    let (db, job_id) = seed_fallback_source(db, source_kind);
    acknowledge_planned_fallback(&db, job_id);
    reinsert_source_after_fallback(&db, job_id);
    ReversedFallbackFixture { db, job_id }
}

fn seed_fallback_source(
    db: Db,
    source_kind: RestoredFallbackSourceKind,
) -> (Db, ReceiverJobId) {
    match source_kind {
        RestoredFallbackSourceKind::FinalAnswer => seed_final_answer_source(db),
        RestoredFallbackSourceKind::ControlAcknowledgement => seed_control_source(db),
        RestoredFallbackSourceKind::UnavailableNotice => seed_unavailable_notice_source(db),
    }
}

fn seed_final_answer_source(db: Db) -> (Db, ReceiverJobId) {
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
    (fixture.db, fixture.job_id)
}

fn seed_control_source(db: Db) -> (Db, ReceiverJobId) {
    let identity = ReceiverConversationIdentity::sms(
        super::support::receiver_workspace_id(),
        super::support::receiver_user_id(),
    );
    let mut inbound = super::support::receiver_job(Some("fallback-control"), 100);
    inbound.prompt = "/new".to_owned();
    let accepted = db
        .accept_receiver_job(&inbound, &identity)
        .expect("accept control job");
    let claim = db
        .claim_next_receiver_run("control-owner", 101, 1_101)
        .expect("claim control job")
        .expect("due control job");
    assert!(
        db.complete_receiver_new_session(accepted.job_id(), claim.claim().owner(), 102)
            .expect("persist control acknowledgement")
    );
    (db, accepted.job_id())
}

fn seed_unavailable_notice_source(db: Db) -> (Db, ReceiverJobId) {
    let identity = ReceiverConversationIdentity::sms(
        super::support::receiver_workspace_id(),
        super::support::receiver_user_id(),
    );
    let inbound = super::support::receiver_job(Some("fallback-unavailable"), 100);
    let accepted = db
        .accept_receiver_job(&inbound, &identity)
        .expect("accept unavailable-notice source");
    db.conn
        .execute(
            "UPDATE receiver_jobs
             SET state = 'failed', pending_unavailable_notice = 1,
                 last_error = 'recovery-attempt-exhausted'
             WHERE job_id = ?1",
            [accepted.job_id().to_string()],
        )
        .expect("stage legacy unavailable notice");
    assert_eq!(
        db.reconcile_expired_receiver_deliveries(200)
            .expect("persist unavailable notice"),
        1
    );
    (db, accepted.job_id())
}

fn acknowledge_planned_fallback(db: &Db, job_id: ReceiverJobId) {
    db.conn
        .execute(
            "UPDATE receiver_deliveries
             SET frozen_fallbacks_json = ?2
             WHERE job_id = ?1 AND response_kind != 'fallback-notice'",
            rusqlite::params![
                job_id.to_string(),
                serde_json::json!([{
                    "provider": "resend",
                    "sender": "brain@example.test",
                    "recipient": "safe@example.test"
                }])
                .to_string(),
            ],
        )
        .expect("freeze authenticated fallback");
    let source_claim = db
        .claim_next_receiver_delivery("source-owner", 2_000, 32_000)
        .expect("claim source response")
        .expect("source response");
    assert!(
        db.mark_receiver_delivery_io_started(&source_claim, 2_100)
            .expect("mark source provider IO")
    );
    assert_eq!(
        db.apply_receiver_delivery_result(
            &source_claim,
            2_200,
            ReceiverProviderResultClass::PermanentlyRejected(
                ReceiverDeliveryErrorCategory::ProviderRejected,
            ),
        )
        .expect("terminalize source response"),
        ReceiverDeliveryApplyOutcome::Applied
    );
    let fallback_claim = db
        .claim_next_receiver_delivery("fallback-owner", 2_300, 32_300)
        .expect("claim fallback notice")
        .expect("fallback notice");
    assert!(
        db.mark_receiver_delivery_io_started(&fallback_claim, 2_400)
            .expect("mark fallback provider IO")
    );
    let reference = ReceiverProviderReference::parse("fallback-provider-reference")
        .expect("provider reference");
    assert!(
        db.apply_receiver_delivery_result(
            &fallback_claim,
            2_500,
            ReceiverProviderResultClass::Acknowledged(reference),
        )
        .expect("acknowledge fallback notice")
            == ReceiverDeliveryApplyOutcome::Applied
    );
}

fn reinsert_source_after_fallback(db: &Db, job_id: ReceiverJobId) {
    let transaction = rusqlite::Transaction::new_unchecked(
        &db.conn,
        rusqlite::TransactionBehavior::Immediate,
    )
    .expect("begin source row reorder");
    transaction
        .execute_batch(
            "CREATE TEMP TABLE reversed_fallback_source AS
               SELECT * FROM receiver_deliveries WHERE 0;",
        )
        .expect("create source row copy");
    assert_eq!(
        transaction
            .execute(
                "INSERT INTO reversed_fallback_source
                 SELECT * FROM receiver_deliveries
                 WHERE job_id = ?1 AND response_kind != 'fallback-notice'",
                [job_id.to_string()],
            )
            .expect("copy terminal source row"),
        1
    );
    assert_eq!(
        transaction
            .execute(
                "DELETE FROM receiver_deliveries
                 WHERE job_id = ?1 AND response_kind != 'fallback-notice'",
                [job_id.to_string()],
            )
            .expect("remove original source row"),
        1
    );
    assert_eq!(
        transaction
            .execute(
                "INSERT INTO receiver_deliveries
                 SELECT * FROM reversed_fallback_source",
                [],
            )
            .expect("reinsert source after fallback"),
        1
    );
    transaction
        .execute_batch("DROP TABLE reversed_fallback_source;")
        .expect("remove source row copy");
    transaction.commit().expect("commit reversed row order");
}

fn assert_reversed_fallback_audit(
    db: &Db,
    job_id: ReceiverJobId,
    source_kind: RestoredFallbackSourceKind,
) {
    let audit: (i64, String, String, Option<String>, String, bool, bool) = db
        .conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM receiver_deliveries AS exact_rows
                     WHERE exact_rows.job_id = source.job_id),
                    source.response_kind, source.state,
                    source.fallback_decision, fallback.state,
                    fallback.provider_reference IS NOT NULL
                      AND length(trim(fallback.provider_reference)) > 0,
                    fallback.rowid < source.rowid
             FROM receiver_deliveries AS source
             JOIN receiver_deliveries AS fallback
               ON fallback.job_id = source.job_id
              AND fallback.job_token = source.job_token
              AND fallback.response_kind = 'fallback-notice'
             WHERE source.job_id = ?1 AND source.response_kind != 'fallback-notice'",
            [job_id.to_string()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .expect("load reversed fallback audit");
    assert_eq!(audit.0, 2, "unexpected row count for {source_kind:?}");
    assert_eq!(audit.1, source_kind.response_kind().as_str());
    assert_eq!(audit.2, "failed");
    assert_eq!(audit.3.as_deref(), Some("fallback-planned"));
    assert_eq!(audit.4, "acknowledged");
    assert!(audit.5, "fallback acknowledgement missing for {source_kind:?}");
    assert!(
        audit.6,
        "fallback row was not inserted before the {source_kind:?} source"
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
fn reversed_fallback_rows_restore_done_on_reopen_and_repeated_repair_for_every_source_kind() {
    for source_kind in RestoredFallbackSourceKind::ALL {
        let temporary = tempfile::tempdir().expect("temporary state directory");
        let path = std::fs::canonicalize(temporary.path())
            .expect("canonical temporary state directory")
            .join("state.db");
        let workspace = super::support::receiver_workspace_id().to_string();
        let actor = super::support::receiver_user_id();
        let fixture = reversed_fallback_fixture(
            &path,
            &workspace,
            actor.as_str(),
            source_kind,
        );
        let job_id = fixture.job_id;
        assert_reversed_fallback_audit(&fixture.db, job_id, source_kind);
        drop(fixture);

        let reopened = Db::open_path_with_legacy_identity(&path, &workspace, actor.as_str())
            .expect("reopen reversed fallback state");
        assert_reversed_fallback_audit(&reopened, job_id, source_kind);
        super::super::schema::up(&reopened.conn, 12).expect("first repeated repair");
        super::super::schema::up(&reopened.conn, 12).expect("second repeated repair");
        assert_reversed_fallback_audit(&reopened, job_id, source_kind);
        assert!(
            reopened
                .claim_next_receiver_delivery("post-repair-owner", 2_600, 32_600)
                .expect("probe post-repair delivery")
                .is_none(),
            "repair recreated {source_kind:?} fallback resend authority"
        );
    }
}

#[test]
fn reversed_fallback_rows_restore_done_across_down_and_reupgrade_for_every_source_kind() {
    for source_kind in RestoredFallbackSourceKind::ALL {
        let temporary = tempfile::tempdir().expect("temporary state directory");
        let path = std::fs::canonicalize(temporary.path())
            .expect("canonical temporary state directory")
            .join("state.db");
        let workspace = super::support::receiver_workspace_id().to_string();
        let actor = super::support::receiver_user_id();
        let fixture = reversed_fallback_fixture(
            &path,
            &workspace,
            actor.as_str(),
            source_kind,
        );
        let job_id = fixture.job_id;
        assert_reversed_fallback_audit(&fixture.db, job_id, source_kind);
        drop(fixture);

        super::super::schema::down_delivery_path(&path)
            .expect("downgrade reversed fallback state");
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
        assert_eq!(version, 11, "wrong downgrade for {source_kind:?}");
        assert_eq!(v11_state, ("done".to_owned(), None));
        assert!(!delivery_table_exists);
        drop(downgraded);

        let upgraded = Db::open_path_with_legacy_identity(&path, &workspace, actor.as_str())
            .expect("re-upgrade reversed fallback state");
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
            "re-upgrade recreated {source_kind:?} fallback resend authority"
        );
    }
}
