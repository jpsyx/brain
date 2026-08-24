#[test]
fn rollback_preserves_the_controller_shutdown_diagnostic_after_cleanup_and_durable_retry() {
    let workspace = workspace();
    let actor = actor();
    let db = Db::open_in_memory().expect("state DB");
    let inbound = inbound(&workspace, &actor);
    let identity = ReceiverConversationIdentity::sms(workspace.id(), actor.user_id().clone());
    let accepted = db
        .accept_receiver_job(&inbound, &identity)
        .expect("accept receiver job");
    let scope = SessionScope::new(AgentKind::Codex, workspace.id(), actor.clone());
    let services = services(db);
    let claimed = services
        .claim_receiver_run("receiver-claim", 1_000, 1_500)
        .expect("claim receiver job")
        .expect("ready receiver job");
    let remote = ReceiverRemoteSession::new("interactive-shell");
    let registration = ReceiverSessionRegistration::register_fresh(
        &services,
        accepted.conversation_id(),
        &remote,
        42,
        &scope,
    )
    .expect("register fresh remote placeholder");
    let shutdowns = Arc::new(Mutex::new(0));
    let mut controller = AgentController::new(
        Arc::clone(&workspace),
        actor,
        Box::new(LaunchFrontend {
            shutdown_diagnostic: Some("receiver shutdown probe failed"),
        }),
        Box::new(ShutdownTransport(Arc::clone(&shutdowns))),
    );

    let error = rollback_receiver_launch(
        &services,
        &claimed,
        Some(registration),
        &mut controller,
        ReceiverLaunchFailure::Planning,
        1_020,
        2_000,
    )
    .expect_err("surface controller shutdown diagnostic");

    assert_eq!(
        error.downcast_ref::<AgentError>(),
        Some(&AgentError::Frontend(
            "receiver shutdown probe failed".to_owned()
        ))
    );
    assert_eq!(*shutdowns.lock().expect("shutdown count"), 1);
    assert!(
        services
            .locked_session_for_instance(remote.instance(), &scope)
            .is_none(),
        "shutdown diagnostics must not skip exact registration cleanup"
    );
    let retry = services
        .claim_receiver_run("retry-owner", 2_000, 2_500)
        .expect("claim due retry")
        .expect("durable retry remains ready");
    assert_eq!(retry.job().state(), ReceiverJobState::Retrying);
    assert_eq!(
        retry.job().last_error(),
        Some(ReceiverLaunchFailure::Planning.as_str())
    );
}

#[test]
fn rollback_surfaces_explicit_registration_cleanup_failure_after_stopping_and_retrying() {
    let workspace = workspace();
    let actor = actor();
    let db = Db::open_in_memory().expect("state DB");
    let inbound_job = inbound(&workspace, &actor);
    let identity = ReceiverConversationIdentity::sms(workspace.id(), actor.user_id().clone());
    db.accept_receiver_job(&inbound_job, &identity)
        .expect("accept receiver job");
    let services = services(db);
    let claimed = services
        .claim_receiver_run("receiver-claim", 1_000, 1_500)
        .expect("claim receiver job")
        .expect("ready receiver job");
    let scope = SessionScope::new(AgentKind::Codex, workspace.id(), actor.clone());
    let cleanup_store = FailingReleaseStore::new();
    let cleanup_inbound = inbound(&workspace, &actor);
    let cleanup_conversation = cleanup_store
        .db()
        .accept_receiver_job(&cleanup_inbound, &identity)
        .expect("accept cleanup-store conversation")
        .conversation_id();
    let remote = ReceiverRemoteSession::new("interactive-shell");
    let registration = ReceiverSessionRegistration::register_fresh(
        &cleanup_store,
        cleanup_conversation,
        &remote,
        42,
        &scope,
    )
    .expect("register cleanup-store placeholder");
    let shutdowns = Arc::new(Mutex::new(0));
    let mut controller = AgentController::new(
        Arc::clone(&workspace),
        actor,
        Box::new(LaunchFrontend {
            shutdown_diagnostic: None,
        }),
        Box::new(ShutdownTransport(Arc::clone(&shutdowns))),
    );

    let error = rollback_receiver_launch(
        &services,
        &claimed,
        Some(registration),
        &mut controller,
        ReceiverLaunchFailure::Planning,
        1_020,
        2_000,
    )
    .expect_err("surface explicit session cleanup failure");

    assert_eq!(error.to_string(), "exact receiver release failed");
    assert_eq!(*shutdowns.lock().expect("shutdown count"), 1);
    let retry = services
        .claim_receiver_run("retry-owner", 2_000, 2_500)
        .expect("claim due retry")
        .expect("durable retry remains ready");
    assert_eq!(retry.job().state(), ReceiverJobState::Retrying);
    assert_eq!(cleanup_store.release_attempts(), 2);
}
