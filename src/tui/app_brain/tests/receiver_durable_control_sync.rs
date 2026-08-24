use super::receiver_durable_support::accept_email_job_in_thread;
use super::receiver_sync::{TestReceiverSyncRuntime, configure_receiver_sync};
use super::*;

use crate::state::ReceiverJobState;

#[test]
fn disabled_pending_new_finishes_its_boundary_without_claiming_following_work() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    configure_receiver_sync(&app);
    let runtime = TestReceiverSyncRuntime::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(runtime.clone()));
    let db = Db::open(app.context.workspace()).expect("state DB");
    let command = accept_email_job_in_thread(&app, &db, "disabled-new", " /NEW\n", 100);
    let following =
        accept_email_job_in_thread(&app, &db, "disabled-new", "after disabled new", 200);
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());

    app.tick_receiver();
    assert_eq!(
        db.receiver_job(command.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Claimed,
        "freshness must hold the claimed control before its durable boundary"
    );
    app.receiver.record_intent(false);
    runtime.finish_pull();

    app.tick_receiver();

    assert_eq!(
        db.receiver_job(command.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Done,
        "the already claimed control must finish while intent is disabled"
    );
    let following_job = db.receiver_job(following.job_id()).unwrap().unwrap();
    assert_ne!(following_job.conversation_id(), command.conversation_id());
    assert_eq!(following_job.state(), ReceiverJobState::Queued);
    assert!(app.brain.receiver_run_observations().is_empty());
    assert!(transport.launch_specs().is_empty());

    app.tick_receiver();
    assert_eq!(
        db.receiver_job(following.job_id())
            .unwrap()
            .unwrap()
            .state(),
        ReceiverJobState::Queued,
        "disabled idle ticks must not claim the following job"
    );
    app.receiver.record_intent(true);

    app.tick_receiver();
    assert_eq!(
        db.receiver_job(following.job_id())
            .unwrap()
            .unwrap()
            .state(),
        ReceiverJobState::Claimed,
        "re-enable returns the job to the ordinary freshness-first path"
    );
    runtime.finish_pull();
    app.tick_receiver();

    assert_eq!(
        db.receiver_job(following.job_id())
            .unwrap()
            .unwrap()
            .state(),
        ReceiverJobState::Launching
    );
    let specs = transport.launch_specs();
    assert_eq!(specs.len(), 1);
    assert!(specs[0].command.contains("after disabled new"));
    assert!(!specs[0].command.contains("/NEW"));
}
