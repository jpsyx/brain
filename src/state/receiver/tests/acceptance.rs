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
    assert!(persisted.inbound() == &job, "persisted inbound job changed");
    assert_eq!(conversation.identity(), &identity);
    assert!(
        conversation.transcript_markdown().is_empty(),
        "new conversation transcript was not empty"
    );
}

#[test]
fn accepted_receiver_job_persists_the_sender_without_breaking_same_version_json() {
    let db = Db::open_in_memory().expect("receiver state");
    let job = receiver_job(Some("provider-frozen-sender"), 100);
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let accepted = db
        .accept_receiver_job(&job, &identity)
        .expect("accept receiver job");
    let (response_sender, inbound_json): (Option<String>, String) = db
        .conn
        .query_row(
            "SELECT response_sender, inbound_json FROM receiver_jobs WHERE job_id = ?1",
            [accepted.job_id().to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("load frozen sender storage");
    let encoded: serde_json::Value =
        serde_json::from_str(&inbound_json).expect("decode compatibility inbound JSON");

    assert!(
        response_sender.as_deref() == Some("+12125550100"),
        "accepted job did not persist its frozen response sender"
    );
    assert!(
        encoded.get("response_sender").is_none(),
        "same-version inbound JSON became unreadable by the prior release"
    );
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
    assert!(
        db.receiver_job(first.job_id())
            .expect("load original")
            .expect("original job")
            .inbound()
            == &original,
        "provider deduplication replaced the original inbound job"
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
fn receiver_acceptance_rebinds_lock_wait_after_database_open() {
    let temporary = tempfile::tempdir().expect("temporary receiver state");
    let path = temporary.path().join("state.db");
    let db = Db::open_path_with_legacy_identity(
        &path,
        &receiver_workspace_id().to_string(),
        receiver_user_id().as_str(),
    )
    .expect("open receiver state with the default lock budget");
    let writer = rusqlite::Connection::open(&path).expect("open competing writer");
    writer
        .execute_batch("BEGIN IMMEDIATE")
        .expect("hold receiver write lock");
    db.rebind_receiver_ingress_busy_timeout(std::time::Duration::from_millis(20))
        .expect("rebind acceptance lock budget");
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());

    let started = std::time::Instant::now();
    let result = db.accept_receiver_job(&receiver_job(None, 100), &identity);

    assert!(result.is_err(), "locked acceptance unexpectedly succeeded");
    assert!(
        started.elapsed() < std::time::Duration::from_millis(500),
        "receiver acceptance inherited the stale pre-open lock budget"
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

#[test]
fn newly_accepted_jobs_receive_distinct_opaque_tokens() {
    let db = Db::open_in_memory().expect("receiver state");
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let first = db
        .accept_receiver_job(&receiver_job(Some("token-first"), 100), &identity)
        .expect("accept first receiver job");
    let second = db
        .accept_receiver_job(&receiver_job(Some("token-second"), 200), &identity)
        .expect("accept second receiver job");

    let first = db
        .receiver_job(first.job_id())
        .expect("load first receiver job")
        .expect("first receiver job");
    let second = db
        .receiver_job(second.job_id())
        .expect("load second receiver job")
        .expect("second receiver job");

    assert_ne!(first.token(), second.token());
    assert!(ReceiverJobToken::parse(&first.token().to_string()).is_ok());
    assert!(ReceiverJobToken::parse("not-a-token").is_err());
}
