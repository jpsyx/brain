#[test]
fn provider_deduplication_is_scoped_by_workspace_and_channel() {
    let temporary = tempfile::tempdir().expect("temporary receiver state");
    let first_path = temporary.path().join("first.db");
    let second_path = temporary.path().join("second.db");
    let first_workspace = receiver_workspace_id();
    let second_workspace = crate::workspace::WorkspaceId::parse(
        "e806258e-491a-436d-9db4-a5ca9903e0d4",
    )
    .expect("valid second workspace ID");
    let first = Db::open_path_with_legacy_identity(
        &first_path,
        &first_workspace.to_string(),
        receiver_user_id().as_str(),
    )
    .expect("open first workspace state");
    let second = Db::open_path_with_legacy_identity(
        &second_path,
        &second_workspace.to_string(),
        receiver_user_id().as_str(),
    )
    .expect("open second workspace state");
    let sms = receiver_job_for(
        first_workspace,
        crate::server::receiver::Channel::Sms,
        Some("shared-provider-id"),
        100,
    );
    let email = receiver_job_for(
        first_workspace,
        crate::server::receiver::Channel::Email,
        Some("shared-provider-id"),
        101,
    );
    let other_workspace = receiver_job_for(
        second_workspace,
        crate::server::receiver::Channel::Sms,
        Some("shared-provider-id"),
        102,
    );

    assert!(
        first
            .accept_receiver_job(
                &sms,
                &ReceiverConversationIdentity::sms(first_workspace, receiver_user_id()),
            )
            .expect("accept SMS")
            .was_inserted()
    );
    assert!(
        first
            .accept_receiver_job(
                &email,
                &ReceiverConversationIdentity::email(
                    first_workspace,
                    receiver_user_id(),
                    EmailLineage::verified("thread-1").expect("verified lineage"),
                ),
            )
            .expect("accept email")
            .was_inserted()
    );
    assert!(
        second
            .accept_receiver_job(
                &other_workspace,
                &ReceiverConversationIdentity::sms(second_workspace, receiver_user_id()),
            )
            .expect("accept other workspace SMS")
            .was_inserted()
    );
}
#[test]
fn conversation_transcript_and_native_binding_update_together() {
    let db = Db::open_in_memory().expect("receiver state");
    let job = receiver_job(None, 100);
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let accepted = db
        .accept_receiver_job(&job, &identity)
        .expect("accept receiver job");
    let binding = ReceiverSessionBinding::new(crate::agent::AgentKind::OpenCode, "session-9")
        .expect("valid binding");

    assert!(
        db.update_receiver_conversation(
            accepted.conversation_id(),
            "# Transcript\n\nUser: hello",
            Some(&binding),
            12_345,
        )
        .expect("update conversation")
    );
    let conversation = db
        .receiver_conversation(accepted.conversation_id())
        .expect("load conversation")
        .expect("conversation");

    assert_eq!(conversation.binding(), Some(&binding));
    assert_eq!(
        conversation.session_plan(crate::agent::AgentKind::OpenCode),
        ReceiverSessionPlan::ResumeNative("session-9".to_owned())
    );
    assert_eq!(
        conversation.session_plan(crate::agent::AgentKind::Claude),
        ReceiverSessionPlan::FreshFromTranscript("# Transcript\n\nUser: hello".to_owned())
    );
    let updated_at: i64 = db
        .conn
        .query_row(
            "SELECT updated_at_unix_ms FROM receiver_conversations
             WHERE conversation_id = ?1",
            [accepted.conversation_id().to_string()],
            |row| row.get(0),
        )
        .expect("conversation update timestamp");
    assert_eq!(updated_at, 12_345);
}

#[test]
fn receiver_acceptance_uses_inbound_millisecond_timestamp_consistently() {
    let db = Db::open_in_memory().expect("receiver state");
    let job = receiver_job(None, 12_345);
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let accepted = db
        .accept_receiver_job(&job, &identity)
        .expect("accept receiver job");

    let timestamps: (i64, i64) = db
        .conn
        .query_row(
            "SELECT created_at_unix_ms, updated_at_unix_ms
             FROM receiver_conversations WHERE conversation_id = ?1",
            [accepted.conversation_id().to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("conversation timestamps");
    assert_eq!(timestamps, (12_345, 12_345));
}
