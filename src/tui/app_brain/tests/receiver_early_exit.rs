use super::receiver_durable_delivery::ScriptedDeliveryExecution;
use super::receiver_durable_support::ReceiverClock;
use super::*;

#[test]
fn exhausted_early_exits_deliver_unavailable_sms_while_the_tui_remains_alive() {
    for kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&temporary, &cli, kind);
        app.receiver.record_intent(true);
        let clock = ReceiverClock::new();
        app.services
            .replace_receiver_sync_runtime(Box::new(clock.clone()));
        let delivery = ScriptedDeliveryExecution::acknowledged();
        app.services
            .replace_receiver_delivery_execution(Box::new(delivery.clone()));
        let db = Db::open(app.context.workspace()).expect("state DB");
        let inbound = receiver_job(&app, sms_actor(), Channel::Sms, "An unaccepted request");
        let identity = crate::state::ReceiverConversationIdentity::sms(
            app.context.workspace().id(),
            inbound.actor.user_id().clone(),
        );
        let accepted = db
            .accept_receiver_job(&inbound, &identity)
            .expect("accept request");

        for _ in 0..3 {
            let transport = TransportRecording::default();
            app.brain.replace_receiver_transport(transport.transport());
            app.tick_receiver();
            assert_eq!(transport.launch_specs().len(), 1, "missing retry launch");
            transport.set_alive(false);
            app.tick_receiver();
            clock.advance(std::time::Duration::from_secs(90));
            app.tick_receiver();
            clock.advance(std::time::Duration::from_secs(5));
        }
        for _ in 0..4 {
            app.tick_receiver();
        }

        assert!(
            db.receiver_job(accepted.job_id()).unwrap().unwrap().state()
                == crate::state::ReceiverJobState::Done,
            "{kind:?} exhausted request never delivered its unavailable notice"
        );
        assert_eq!(
            delivery.start_count(),
            1,
            "exactly one failure notice must reach the provider"
        );
        let conn =
            rusqlite::Connection::open(app.context.state_db_path()).expect("inspect delivery");
        let kind: String = conn
            .query_row(
                "SELECT response_kind FROM receiver_deliveries WHERE job_id = ?1",
                [accepted.job_id().to_string()],
                |row| row.get(0),
            )
            .expect("failure delivery");
        assert!(
            kind == "unavailable-notice",
            "wrong response kind after exhaustion"
        );
    }
}

#[test]
fn exited_receiver_keeps_cleanup_proof_when_session_lookup_fails() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let db = Db::open(app.context.workspace()).expect("state DB");
    super::receiver_durable_support::accept_email_job(&app, &db, "Early exit", 100);
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    app.tick_receiver();
    transport.set_alive(false);
    app.tick_receiver();
    let conn = rusqlite::Connection::open(app.context.state_db_path()).expect("fault connection");
    conn.execute_batch("ALTER TABLE brain_sessions RENAME TO unavailable_sessions")
        .expect("simulate unavailable lifecycle store");

    app.tick_receiver();

    conn.execute_batch("ALTER TABLE unavailable_sessions RENAME TO brain_sessions")
        .expect("restore lifecycle store");
    assert_eq!(
        app.brain.receiver_run_observations().len(),
        1,
        "store failure discarded confirmed local shutdown proof"
    );
    assert_eq!(transport.shutdowns(), 1);
}

#[test]
fn competing_requeue_retires_the_stopped_tab_without_blocking_the_next_attempt() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(app.context.workspace()).expect("state DB");
    super::receiver_durable_support::accept_email_job(&app, &db, "Early exit", 100);
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    app.tick_receiver();
    transport.set_alive(false);
    app.tick_receiver();
    clock.advance(std::time::Duration::from_secs(90));
    db.reconcile_next_receiver_job(clock.unix_ms())
        .expect("competing reconciler")
        .expect("retry effect");

    app.tick_receiver();

    assert!(app.brain.receiver_run_observations().is_empty());
    assert_eq!(transport.shutdowns(), 1);
    clock.advance(std::time::Duration::from_secs(5));
    let retry = TransportRecording::default();
    app.brain.replace_receiver_transport(retry.transport());
    app.tick_receiver();
    assert_eq!(retry.launch_specs().len(), 1);
}
