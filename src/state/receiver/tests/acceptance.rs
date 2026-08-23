#[test]
fn accepted_receiver_job_and_conversation_survive_database_reopen() {
    let temporary = tempfile::tempdir().expect("temporary receiver state");
    let path = temporary.path().join("state.db");
    let job = receiver_job(Some("provider-1"), 100);
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());

    let accepted = {
        let db = Db::open_path_with_legacy_identity(
            &path,
            &receiver_workspace_id().to_string(),
            receiver_user_id().as_str(),
        )
        .expect("open receiver state");
        db.accept_receiver_job(&job, &identity)
            .expect("accept durable receiver job")
    };
    let reopened = Db::open_path_with_legacy_identity(
        &path,
        &receiver_workspace_id().to_string(),
        receiver_user_id().as_str(),
    )
    .expect("reopen receiver state");

    let persisted = reopened
        .receiver_job(accepted.job_id())
        .expect("load durable job")
        .expect("job remains present");
    let conversation = reopened
        .receiver_conversation(accepted.conversation_id())
        .expect("load durable conversation")
        .expect("conversation remains present");
    assert!(accepted.was_inserted());
    assert_eq!(persisted.state(), ReceiverJobState::Queued);
    assert_eq!(persisted.inbound(), &job);
    assert_eq!(conversation.identity(), &identity);
    assert_eq!(conversation.transcript_markdown(), "");
}

#[test]
fn provider_delivery_id_deduplicates_without_replacing_the_original_job() {
    let db = Db::open_in_memory().expect("receiver state");
    let original = receiver_job(Some("provider-duplicate"), 100);
    let duplicate = receiver_job(Some("provider-duplicate"), 200);
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());

    let first = db
        .accept_receiver_job(&original, &identity)
        .expect("accept original");
    let second = db
        .accept_receiver_job(&duplicate, &identity)
        .expect("deduplicate retry");

    assert!(first.was_inserted());
    assert!(!second.was_inserted());
    assert_eq!(second.job_id(), first.job_id());
    assert_eq!(
        db.receiver_job(first.job_id())
            .expect("load original")
            .expect("original job")
            .inbound(),
        &original
    );
}

#[test]
fn receiver_database_rejects_an_inbound_job_from_another_workspace() {
    let db = Db::open_in_memory().expect("receiver state");
    let other_workspace = crate::workspace::WorkspaceId::parse(
        "38bf600c-dbf6-4e78-b793-863426665f5f",
    )
    .expect("other workspace ID");
    let job = receiver_job_for(
        other_workspace,
        crate::server::receiver::Channel::Sms,
        None,
        100,
    );
    let identity = ReceiverConversationIdentity::sms(other_workspace, receiver_user_id());

    let error = db
        .accept_receiver_job(&job, &identity)
        .expect_err("reject another workspace's inbound job");

    assert_eq!(
        error.to_string(),
        "receiver job belongs to another workspace"
    );
}

#[test]
fn receiver_database_rejects_a_conversation_identity_from_another_workspace() {
    let db = Db::open_in_memory().expect("receiver state");
    let job = receiver_job(None, 100);
    let other_workspace = crate::workspace::WorkspaceId::parse(
        "38bf600c-dbf6-4e78-b793-863426665f5f",
    )
    .expect("other workspace ID");
    let identity = ReceiverConversationIdentity::sms(other_workspace, receiver_user_id());

    let error = db
        .accept_receiver_job(&job, &identity)
        .expect_err("reject another workspace's conversation identity");

    assert_eq!(
        error.to_string(),
        "receiver conversation belongs to another workspace"
    );
}
