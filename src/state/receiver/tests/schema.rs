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
