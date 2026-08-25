use super::receiver_durable_support::{ReceiverClock, accept_email_job};
use super::*;

use crate::server::receiver::AttachmentRef;
use crate::state::ReceiverConversationIdentity;
use crate::state::ReceiverJobState;

use super::receiver_attachment_worker_support::ControlledAttachmentWorker;

#[test]
fn orderly_shell_shutdown_releases_the_active_receiver_and_records_one_fresh_retry() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "stop during receiver launch", 100);
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    app.tick_receiver();
    let attribution = app
        .receiver
        .active_durable_run()
        .expect("active receiver")
        .attribution
        .clone();
    let artifact = app
        .context
        .workspace()
        .paths()
        .responses_dir()
        .join(format!("{}.json", attribution.instance()));
    std::fs::create_dir_all(artifact.parent().expect("response directory"))
        .expect("create response directory");
    std::fs::write(&artifact, "partial private response").expect("partial response artifact");
    let before = (
        app.shell.main_view(),
        app.effective_brain_tab(),
        app.shell.focus(),
    );

    app.shutdown_receiver_runtime();
    assert!(app.shutdown_agent_controllers().is_empty());
    app.shutdown_receiver_runtime();
    assert!(app.shutdown_agent_controllers().is_empty());

    let job = db.receiver_job(accepted.job_id()).unwrap().unwrap();
    assert_eq!(job.state(), ReceiverJobState::Retrying);
    assert_eq!(job.retry_count(), 1);
    assert_eq!(job.retry_at_unix_ms(), Some(clock.unix_ms() + 5_000));
    assert!(
        app.services
            .locked_session_for_instance(attribution.instance(), attribution.scope())
            .is_none()
    );
    assert!(app.brain.receiver_run_observations().is_empty());
    assert_eq!(transport.shutdowns(), 1);
    assert!(!artifact.exists());
    assert_eq!(
        (
            app.shell.main_view(),
            app.effective_brain_tab(),
            app.shell.focus(),
        ),
        before
    );
}

#[test]
fn orderly_shell_shutdown_after_owner_loss_performs_only_local_cleanup() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::OpenCode);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "lose owner before shutdown", 100);
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    app.tick_receiver();
    let attribution = app
        .receiver
        .active_durable_run()
        .expect("active receiver")
        .attribution
        .clone();
    clock.advance(std::time::Duration::from_secs(31));
    let now = clock.unix_ms();
    db.claim_next_receiver_run("replacement-owner", now, now + 30_000)
        .expect("replacement claim")
        .expect("expired launch is reclaimable");
    let after_replacement = db
        .receiver_job(accepted.job_id())
        .expect("load replacement state")
        .expect("replacement job");

    app.shutdown_receiver_runtime();
    assert!(app.shutdown_agent_controllers().is_empty());

    let job = db.receiver_job(accepted.job_id()).unwrap().unwrap();
    assert_eq!(job, after_replacement);
    assert!(
        app.services
            .locked_session_for_instance(attribution.instance(), attribution.scope())
            .is_none(),
        "the replacement recovery owns exact stale lifecycle cleanup"
    );
    assert!(app.brain.receiver_run_observations().is_empty());
    assert_eq!(transport.shutdowns(), 1);
}

#[test]
fn orderly_shell_shutdown_cancels_claimed_staging_and_records_one_planning_retry() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Codex);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let mut inbound = receiver_job(&app, sms_actor(), Channel::Sms, "stage before shutdown");
    inbound.attachments = vec![AttachmentRef {
        url: "https://media.example.test/private".to_owned(),
        provider_id: None,
        content_type: Some("text/plain".to_owned()),
        filename: Some("private.txt".to_owned()),
    }];
    let identity = ReceiverConversationIdentity::sms(
        app.context.workspace().id(),
        inbound.actor.user_id().clone(),
    );
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = db
        .accept_receiver_job(&inbound, &identity)
        .expect("accept receiver job");
    let worker = ControlledAttachmentWorker::default();
    app.services
        .replace_receiver_attachment_runtime(Box::new(worker.clone()));

    app.tick_receiver();
    assert_eq!(worker.starts(), 1);
    app.shutdown_receiver_runtime();
    app.shutdown_receiver_runtime();

    let job = db.receiver_job(accepted.job_id()).unwrap().unwrap();
    assert_eq!(job.state(), ReceiverJobState::Retrying);
    assert_eq!(job.retry_count(), 1);
    assert_eq!(job.retry_at_unix_ms(), Some(clock.unix_ms() + 5_000));
    assert_eq!(job.last_error(), Some("launch-planning"));
    assert_eq!(worker.cancellations(), 1);
    assert_eq!(worker.shutdowns(), 1);
}
