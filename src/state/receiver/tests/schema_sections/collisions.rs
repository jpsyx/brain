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
