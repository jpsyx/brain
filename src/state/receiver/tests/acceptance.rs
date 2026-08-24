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
fn queued_capacity_rejects_new_work_but_allows_provider_retry_deduplication() {
    let db = Db::open_in_memory().expect("receiver state");
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let mut first = None;
    for index in 0..64 {
        let provider_id = format!("provider-capacity-{index}");
        let job = receiver_job(Some(&provider_id), 100 + index);
        let accepted = db
            .accept_receiver_job(&job, &identity)
            .expect("capacity should accept the first 64 queued jobs");
        first.get_or_insert((job, accepted));
    }

    let overflow = receiver_job(Some("provider-capacity-overflow"), 1_000);
    let error = db
        .accept_receiver_job(&overflow, &identity)
        .expect_err("capacity must reject a new queued job");
    assert_eq!(
        error.to_string(),
        "receiver queued-job capacity of 64 is full"
    );

    let (original, original_acceptance) = first.expect("first accepted job");
    let mut provider_retry = receiver_job(original.provider_id.as_deref(), 2_000);
    provider_retry.prompt = "provider retry after response loss".to_owned();
    let duplicate = db
        .accept_receiver_job(&provider_retry, &identity)
        .expect("a provider retry must resolve before the capacity check");
    assert!(!duplicate.was_inserted());
    assert_eq!(duplicate.job_id(), original_acceptance.job_id());
}

#[test]
fn concurrent_admission_cannot_overbook_the_last_queued_slot() {
    let temporary = tempfile::tempdir().expect("temporary receiver state");
    let path = temporary.path().join("state.db");
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let seed = Db::open_path_with_legacy_identity(
        &path,
        &receiver_workspace_id().to_string(),
        receiver_user_id().as_str(),
    )
    .expect("open receiver state");
    for index in 0..63 {
        let provider_id = format!("provider-concurrent-seed-{index}");
        seed.accept_receiver_job(&receiver_job(Some(&provider_id), 100 + index), &identity)
            .expect("seed queued capacity");
    }
    drop(seed);

    let contenders = 8;
    let start = std::sync::Arc::new(std::sync::Barrier::new(contenders));
    #[expect(
        clippy::needless_collect,
        reason = "all barrier contenders must start before any join"
    )]
    let handles = (0..contenders)
        .map(|index| {
            let db = Db::open_path_with_legacy_identity(
                &path,
                &receiver_workspace_id().to_string(),
                receiver_user_id().as_str(),
            )
            .expect("open contender state");
            let start = std::sync::Arc::clone(&start);
            let identity = identity.clone();
            std::thread::spawn(move || {
                let provider_id = format!("provider-concurrent-{index}");
                let job = receiver_job(Some(&provider_id), 1_000 + index as u64);
                start.wait();
                db.accept_receiver_job(&job, &identity)
            })
        })
        .collect::<Vec<_>>();

    let results = handles
        .into_iter()
        .map(|handle| handle.join().expect("admission contender"))
        .collect::<Vec<_>>();
    let inserted = results
        .iter()
        .filter(|result| result.as_ref().is_ok_and(|accepted| accepted.was_inserted()))
        .count();
    let capacity_rejections = results
        .iter()
        .filter(|result| {
            result
                .as_ref()
                .is_err_and(|error| {
                    error.to_string() == "receiver queued-job capacity of 64 is full"
                })
        })
        .count();

    assert_eq!(inserted, 1, "exactly one contender should fill slot 64");
    assert_eq!(
        capacity_rejections,
        contenders - 1,
        "every losing contender should observe durable capacity: {results:?}"
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
