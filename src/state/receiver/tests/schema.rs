#[test]
fn receiver_schema_enforces_conversation_foreign_keys() {
    let db = Db::open_in_memory().expect("receiver state");
    let enabled: i64 = db
        .conn
        .pragma_query_value(None, "foreign_keys", |row| row.get(0))
        .expect("foreign key setting");
    let job = receiver_job(None, 100);
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let accepted = db
        .accept_receiver_job(&job, &identity)
        .expect("accept receiver job");

    assert_eq!(enabled, 1);
    assert!(
        db.conn
            .execute(
                "DELETE FROM receiver_conversations WHERE conversation_id = ?1",
                [accepted.conversation_id().to_string()],
            )
            .is_err()
    );
}

#[test]
fn v6_upgrade_repairs_missing_receiver_state_before_advancing_to_v8() {
    let db = Db::open_in_memory().expect("receiver state");
    db.conn
        .execute_batch("DROP TABLE receiver_jobs; PRAGMA user_version = 6;")
        .expect("seed partial v6 schema");

    super::super::schema::up(&db.conn, 6).expect("repair and upgrade receiver schema");

    let version: i64 = db
        .conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("state schema version");
    let retry_origin_columns: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('receiver_jobs')
             WHERE name = 'retry_from_state'",
            [],
            |row| row.get(0),
        )
        .expect("receiver retry-origin column count");
    let registration_tables: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'receiver_session_registrations'",
            [],
            |row| row.get(0),
        )
        .expect("receiver registration table count");
    assert_eq!(version, 8);
    assert_eq!(retry_origin_columns, 1);
    assert_eq!(registration_tables, 1);
}
