fn accept_jobs_with_ids(db: &Db, jobs: &[(&str, &str)]) {
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    for &(provider_id, job_id) in jobs {
        let mut job = receiver_job(Some(provider_id), 100);
        job.job_id = uuid::Uuid::parse_str(job_id).expect("fixed job ID");
        db.accept_receiver_job(&job, &identity)
            .expect("accept receiver job");
    }
}

fn job_tokens_in_id_order(db: &Db) -> Vec<String> {
    db.conn
        .prepare("SELECT job_token FROM receiver_jobs ORDER BY job_id")
        .expect("prepare migrated token query")
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query migrated tokens")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("collect migrated tokens")
}

fn optional_job_tokens_in_id_order(db: &Db) -> Vec<(String, Option<String>)> {
    db.conn
        .prepare("SELECT job_id, job_token FROM receiver_jobs ORDER BY job_id")
        .expect("prepare partial token query")
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("query partial tokens")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("collect partial tokens")
}

fn receiver_jobs_sql(db: &Db) -> String {
    db.conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'receiver_jobs'",
            [],
            |row| row.get(0),
        )
        .expect("receiver jobs schema")
}

#[test]
fn repeated_token_exhaustion_rolls_back_v8_reconciliation() {
    let db = Db::open_in_memory().expect("receiver state");
    accept_jobs_with_ids(
        &db,
        &[
            (
                "exhaustion-first",
                "10000000-0000-4000-8000-000000000001",
            ),
            (
                "exhaustion-second",
                "20000000-0000-4000-8000-000000000002",
            ),
            (
                "exhaustion-reserved",
                "30000000-0000-4000-8000-000000000003",
            ),
        ],
    );
    stage_v8_receiver_jobs(&db);
    let reserved = ReceiverJobToken::parse("40000000-0000-4000-8000-000000000004")
        .expect("reserved partial token");
    db.conn
        .execute_batch(&format!(
            "ALTER TABLE receiver_jobs ADD COLUMN job_token TEXT;
             UPDATE receiver_jobs SET job_token = '{reserved}'
             WHERE job_id = '30000000-0000-4000-8000-000000000003';"
        ))
        .expect("stage one reserved token");
    let replacement = ReceiverJobToken::parse("50000000-0000-4000-8000-000000000005")
        .expect("first replacement token");
    let before_schema = receiver_jobs_sql(&db);
    let before_tokens = optional_job_tokens_in_id_order(&db);
    let before_version: i64 = db
        .conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("partial schema version");
    let mut generated = 0_usize;

    let error = super::super::schema::up_with_token_factory(&db.conn, 8, || {
        generated += 1;
        assert!(
            generated <= 4,
            "token factory exceeded the expected finite allocation budget"
        );
        if generated == 1 { replacement } else { reserved }
    })
    .expect_err("repeating token generation must exhaust its allocation budget");

    assert_eq!(generated, 4);
    assert_eq!(
        error.to_string(),
        "receiver job token allocation exhausted for job \
         20000000-0000-4000-8000-000000000002 after 3 attempts"
    );
    assert_eq!(receiver_jobs_sql(&db), before_schema);
    assert_eq!(optional_job_tokens_in_id_order(&db), before_tokens);
    assert_eq!(
        db.conn
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .expect("rolled-back schema version"),
        before_version
    );
}

#[test]
fn v8_upgrade_retries_a_colliding_token_candidate() {
    let db = Db::open_in_memory().expect("receiver state");
    accept_jobs_with_ids(
        &db,
        &[
            (
                "collision-first",
                "10000000-0000-4000-8000-000000000001",
            ),
            (
                "collision-second",
                "20000000-0000-4000-8000-000000000002",
            ),
        ],
    );
    stage_v8_receiver_jobs(&db);
    let repeated = ReceiverJobToken::parse("30000000-0000-4000-8000-000000000003")
        .expect("repeated token candidate");
    let replacement = ReceiverJobToken::parse("40000000-0000-4000-8000-000000000004")
        .expect("replacement token candidate");
    let mut candidates = [repeated, repeated, replacement].into_iter();

    super::super::schema::up_with_token_factory(&db.conn, 8, || {
        candidates.next().expect("bounded token candidate")
    })
    .expect("upgrade v8 receiver jobs after token collision");

    assert_eq!(
        job_tokens_in_id_order(&db),
        vec![repeated.to_string(), replacement.to_string()]
    );
}

#[test]
fn partial_v8_duplicate_tokens_keep_the_lowest_job_id_token() {
    let db = Db::open_in_memory().expect("receiver state");
    accept_jobs_with_ids(
        &db,
        &[
            (
                "duplicate-first",
                "10000000-0000-4000-8000-000000000001",
            ),
            (
                "duplicate-second",
                "20000000-0000-4000-8000-000000000002",
            ),
        ],
    );
    stage_v8_receiver_jobs(&db);
    let duplicate = ReceiverJobToken::parse("30000000-0000-4000-8000-000000000003")
        .expect("duplicate partial token");
    db.conn
        .execute_batch(&format!(
            "ALTER TABLE receiver_jobs ADD COLUMN job_token TEXT;
             UPDATE receiver_jobs SET job_token = '{duplicate}';"
        ))
        .expect("stage duplicate partial tokens");
    let replacement = ReceiverJobToken::parse("40000000-0000-4000-8000-000000000004")
        .expect("replacement token");

    super::super::schema::up_with_token_factory(&db.conn, 8, || replacement)
        .expect("reconcile partial v8 token collision");

    assert_eq!(
        job_tokens_in_id_order(&db),
        vec![duplicate.to_string(), replacement.to_string()]
    );
}

#[test]
fn partial_v8_generated_candidates_do_not_replace_existing_tokens() {
    let db = Db::open_in_memory().expect("receiver state");
    accept_jobs_with_ids(
        &db,
        &[
            (
                "reserved-first",
                "10000000-0000-4000-8000-000000000001",
            ),
            (
                "reserved-second",
                "20000000-0000-4000-8000-000000000002",
            ),
        ],
    );
    stage_v8_receiver_jobs(&db);
    let reserved = ReceiverJobToken::parse("30000000-0000-4000-8000-000000000003")
        .expect("reserved partial token");
    db.conn
        .execute_batch(&format!(
            "ALTER TABLE receiver_jobs ADD COLUMN job_token TEXT;
             UPDATE receiver_jobs SET job_token = '{reserved}'
             WHERE job_id = '20000000-0000-4000-8000-000000000002';"
        ))
        .expect("stage reserved partial token");
    let replacement = ReceiverJobToken::parse("40000000-0000-4000-8000-000000000004")
        .expect("replacement token");
    let mut candidates = [reserved, replacement].into_iter();

    super::super::schema::up_with_token_factory(&db.conn, 8, || {
        candidates.next().expect("bounded token candidate")
    })
    .expect("reconcile partial v8 token reservation");

    assert_eq!(
        job_tokens_in_id_order(&db),
        vec![replacement.to_string(), reserved.to_string()]
    );
}

#[test]
fn current_v9_reconciliation_canonicalizes_and_repairs_semantic_uuid_collisions() {
    let db = Db::open_in_memory().expect("receiver state");
    accept_jobs_with_ids(
        &db,
        &[
            (
                "semantic-uppercase",
                "10000000-0000-4000-8000-000000000001",
            ),
            (
                "semantic-canonical",
                "20000000-0000-4000-8000-000000000002",
            ),
            (
                "semantic-invalid",
                "30000000-0000-4000-8000-000000000003",
            ),
            (
                "semantic-reserved",
                "40000000-0000-4000-8000-000000000004",
            ),
        ],
    );
    let duplicate = ReceiverJobToken::parse("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa")
        .expect("duplicate token");
    let reserved = ReceiverJobToken::parse("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb")
        .expect("reserved token");
    db.conn
        .execute_batch(&format!(
            "UPDATE receiver_jobs SET job_token = '{}'
               WHERE job_id = '10000000-0000-4000-8000-000000000001';
             UPDATE receiver_jobs SET job_token = '{duplicate}'
               WHERE job_id = '20000000-0000-4000-8000-000000000002';
             UPDATE receiver_jobs SET job_token = 'not-a-uuid'
               WHERE job_id = '30000000-0000-4000-8000-000000000003';
             UPDATE receiver_jobs SET job_token = '{reserved}'
               WHERE job_id = '40000000-0000-4000-8000-000000000004';",
            duplicate.to_string().to_uppercase(),
        ))
        .expect("stage semantically damaged v9 tokens");
    let first_replacement =
        ReceiverJobToken::parse("cccccccc-cccc-4ccc-8ccc-cccccccccccc").unwrap();
    let second_replacement =
        ReceiverJobToken::parse("dddddddd-dddd-4ddd-8ddd-dddddddddddd").unwrap();
    let mut candidates = [reserved, first_replacement, second_replacement].into_iter();

    super::super::schema::up_with_token_factory(&db.conn, 9, || {
        candidates.next().expect("bounded token candidate")
    })
    .expect("repair current v9 token identities");

    let expected = vec![
        first_replacement.to_string(),
        duplicate.to_string(),
        second_replacement.to_string(),
        reserved.to_string(),
    ];
    assert_eq!(job_tokens_in_id_order(&db), expected);
    super::super::schema::up_with_token_factory(&db.conn, 9, || {
        panic!("idempotent canonical rows must not allocate another token")
    })
    .expect("reconcile canonical v9 tokens idempotently");
    assert_eq!(job_tokens_in_id_order(&db), expected);
}
