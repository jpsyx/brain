fn assert_malformed_v11_shape_rolls_back_delivery_downgrade(alter_schema: &str) {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = temporary.path().join("state.db");
    let job_id = {
        let db = Db::open_path_with_legacy_identity(
            &path,
            &receiver_workspace_id().to_string(),
            receiver_user_id().as_str(),
        )
        .expect("receiver state");
        let accepted = db
            .accept_receiver_job(
                &receiver_job(Some("private-malformed-v11"), 100),
                &ReceiverConversationIdentity::sms(
                    receiver_workspace_id(),
                    receiver_user_id(),
                ),
            )
            .expect("accept receiver job");
        db.conn
            .execute(
                "UPDATE receiver_jobs SET state = 'answer-ready' WHERE job_id = ?1",
                [accepted.job_id().to_string()],
            )
            .expect("stage answer-ready job");
        seed_delivery_row(
            &db,
            accepted.job_id(),
            persisted_job_token(&db, accepted.job_id()),
            "10000000-0000-4000-8000-000000000001",
            "final-answer",
            "ready",
            r#"{"channel":"sms","value":{"recipient":"+12125550199","body":"private answer","long_form_available":false}}"#,
        );
        db.conn
            .execute_batch(alter_schema)
            .expect("damage one v11 schema requirement");
        accepted.job_id().to_string()
    };

    let error = super::super::schema::down_delivery_path(&path)
        .expect_err("malformed v11 schema must block downgrade");

    let connection = rusqlite::Connection::open(path).expect("unchanged v12 state");
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("retained version");
    let outbox_rows: i64 = connection
        .query_row("SELECT COUNT(*) FROM receiver_deliveries", [], |row| {
            row.get(0)
        })
        .expect("retained outbox rows");
    let job_state: String = connection
        .query_row(
            "SELECT state FROM receiver_jobs WHERE job_id = ?1",
            [job_id],
            |row| row.get(0),
        )
        .expect("retained job state");

    assert_eq!(version, 12);
    assert_eq!(outbox_rows, 1);
    assert_eq!(job_state, "answer-ready");
    assert!(!error.to_string().contains("private"));
}

#[test]
fn v12_down_requires_the_complete_v11_conversation_shape() {
    assert_malformed_v11_shape_rolls_back_delivery_downgrade(
        "ALTER TABLE receiver_conversations DROP COLUMN transcript_markdown;",
    );
}

#[test]
fn v12_down_requires_the_complete_v11_job_recovery_shape() {
    assert_malformed_v11_shape_rolls_back_delivery_downgrade(
        "ALTER TABLE receiver_jobs DROP COLUMN latest_progress_at_unix_ms;",
    );
}

#[test]
fn v12_down_requires_the_complete_v11_job_notice_shape() {
    assert_malformed_v11_shape_rolls_back_delivery_downgrade(
        "ALTER TABLE receiver_jobs DROP COLUMN unavailable_notice_expires_at_unix_ms;",
    );
}

#[test]
fn v12_down_requires_the_complete_v11_registration_shape() {
    assert_malformed_v11_shape_rolls_back_delivery_downgrade(
        "ALTER TABLE receiver_session_registrations DROP COLUMN actual_session_id;",
    );
}
