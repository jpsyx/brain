use super::*;

use crate::state::{EmailLineage, ReceiverConversationIdentity, ReceiverJobState};

#[test]
fn restart_committed_after_empty_scan_blocks_the_same_tick_ordinary_claim() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let db = Db::open(app.context.workspace()).expect("state DB");
    let older = accepted_thread_job(&app, &db, "race-thread", "older backlog", 100);
    let (restart, identity) = thread_job(&app, "race-thread", " /ReStArT\n", 200);
    let ingress_db = Db::open_path_with_legacy_identity(
        app.context.state_db_path(),
        &app.context.workspace().id().to_string(),
        app.context.workspace().local_user_id(),
    )
    .expect("provider ingress DB");
    let scan_finished = Arc::new(std::sync::Barrier::new(2));
    let ingress_committed = Arc::new(std::sync::Barrier::new(2));
    let provider_scan_finished = Arc::clone(&scan_finished);
    let provider_ingress_committed = Arc::clone(&ingress_committed);
    let provider = std::thread::spawn(move || {
        provider_scan_finished.wait();
        let accepted = ingress_db
            .accept_receiver_job(&restart, &identity)
            .expect("commit restart after empty scan");
        provider_ingress_committed.wait();
        accepted
    });
    app.receiver
        .install_after_restart_scan_hook(Box::new(move || {
            scan_finished.wait();
            ingress_committed.wait();
        }));
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());

    app.tick_receiver();
    let restart = provider.join().expect("provider ingress thread");

    assert!(app.brain.receiver_run_observations().is_empty());
    assert!(transport.launch_specs().is_empty());
    assert_eq!(
        db.receiver_job(older.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Queued,
        "backlog before the raced restart must not become active"
    );
    assert_eq!(
        db.receiver_job(restart.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Queued,
        "the literal restart remains ready for the next control scan"
    );

    app.tick_receiver();

    assert_eq!(
        db.receiver_job(older.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Failed
    );
    assert_eq!(
        db.receiver_job(restart.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Done
    );
    assert!(app.brain.receiver_run_observations().is_empty());
    assert!(transport.launch_specs().is_empty());
}

fn accepted_thread_job(
    app: &App,
    db: &Db,
    thread: &str,
    prompt: &str,
    received_at_unix_ms: u64,
) -> crate::state::ReceiverAcceptance {
    let (job, identity) = thread_job(app, thread, prompt, received_at_unix_ms);
    db.accept_receiver_job(&job, &identity)
        .expect("accept durable thread job")
}

fn thread_job(
    app: &App,
    thread: &str,
    prompt: &str,
    received_at_unix_ms: u64,
) -> (InboundJob, ReceiverConversationIdentity) {
    let mut inbound = receiver_job(app, email_actor(), Channel::Email, prompt);
    inbound.job_id = uuid::Uuid::new_v4();
    inbound.received_at_unix_ms = received_at_unix_ms;
    inbound.provider_id = Some(format!("provider-{}", inbound.job_id));
    inbound.authenticated_sender = "member@example.test".to_owned();
    inbound.thread_participants = vec!["member@example.test".to_owned()];
    let identity = ReceiverConversationIdentity::email(
        app.context.workspace().id(),
        inbound.actor.user_id().clone(),
        EmailLineage::verified(thread).expect("verified thread"),
    );
    (inbound, identity)
}
