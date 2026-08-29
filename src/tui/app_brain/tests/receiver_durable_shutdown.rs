use super::receiver_durable_support::{ReceiverClock, accept_email_job};
use super::*;

use crate::server::receiver::{AttachmentRef, StagedAttachment};
use crate::state::ReceiverConversationIdentity;
use crate::state::ReceiverJobState;

use super::receiver_attachment_worker_support::ControlledAttachmentWorker;

#[test]
fn orderly_shell_shutdown_cleans_launched_run_without_replay_or_correlation_loss() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services.replace_receiver_sync_runtime(Box::new(clock));
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
    let durable_before = db
        .receiver_job(accepted.job_id())
        .expect("load launched job")
        .expect("launched job");
    let correlation_before = app
        .services
        .locked_session_for_instance(attribution.instance(), attribution.scope());
    let before = (
        app.shell.main_view(),
        app.effective_brain_tab(),
        app.shell.focus(),
    );

    app.shutdown_receiver_runtime();
    assert!(app.shutdown_agent_controllers().is_empty());
    app.shutdown_receiver_runtime();
    assert!(app.shutdown_agent_controllers().is_empty());

    assert_eq!(
        db.receiver_job(accepted.job_id()).unwrap().unwrap(),
        durable_before,
        "orderly shutdown must not reinterpret a launched run as pre-spawn failure"
    );
    assert_eq!(
        app.services
            .locked_session_for_instance(attribution.instance(), attribution.scope()),
        correlation_before,
        "orderly shutdown must retain durable session correlation"
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
fn orderly_shutdown_after_lease_expiry_preserves_launched_job_and_correlation() {
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
    let durable_before = db
        .receiver_job(accepted.job_id())
        .expect("load launched job")
        .expect("launched job");
    let correlation_before = app
        .services
        .locked_session_for_instance(attribution.instance(), attribution.scope());
    clock.advance(std::time::Duration::from_secs(31));

    app.shutdown_receiver_runtime();
    assert!(app.shutdown_agent_controllers().is_empty());
    let after_first_shutdown = db
        .receiver_job(accepted.job_id())
        .expect("load first shutdown state")
        .expect("job after first shutdown");
    app.shutdown_receiver_runtime();
    assert!(app.shutdown_agent_controllers().is_empty());

    let job = db.receiver_job(accepted.job_id()).unwrap().unwrap();
    assert_eq!(job, after_first_shutdown);
    assert!(job == durable_before, "shutdown changed the durable job");
    assert_eq!(
        app.services
            .locked_session_for_instance(attribution.instance(), attribution.scope()),
        correlation_before
    );
    assert!(app.brain.receiver_run_observations().is_empty());
    assert_eq!(transport.shutdowns(), 1);
}

#[test]
fn orderly_active_shutdown_cleans_attachments_without_replaying_launched_work() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let mut inbound = receiver_job(&app, sms_actor(), Channel::Sms, "launch with attachment");
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
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());

    app.tick_receiver();
    let stage = worker.stage(0);
    let directory = app
        .context
        .workspace()
        .paths()
        .inbox_dir()
        .join("active-shutdown-cleanup");
    std::fs::create_dir_all(&directory).expect("attachment directory");
    let attachment_path = directory.join("private.txt");
    std::fs::write(&attachment_path, b"private attachment").expect("attachment file");
    worker.complete_with_cleanup_clock(
        stage,
        directory.clone(),
        vec![StagedAttachment {
            source: "provider-attachment".to_owned(),
            path: Some(attachment_path),
            error: None,
        }],
        clock,
        std::time::Duration::from_secs(7),
    );
    app.tick_receiver();
    assert!(app.receiver.active_durable_run().is_some());
    let durable_before = db
        .receiver_job(accepted.job_id())
        .expect("load launched job")
        .expect("launched job");

    app.shutdown_receiver_runtime();

    let job = db.receiver_job(accepted.job_id()).unwrap().unwrap();
    assert!(job == durable_before, "shutdown changed the durable job");
    assert!(!directory.exists());
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
    worker.advance_on_shutdown(clock.clone(), std::time::Duration::from_secs(7));

    app.tick_receiver();
    assert_eq!(worker.starts(), 1);
    app.shutdown_receiver_runtime();
    app.shutdown_receiver_runtime();

    let job = db.receiver_job(accepted.job_id()).unwrap().unwrap();
    assert_eq!(job.state(), ReceiverJobState::Retrying);
    assert!(
        job.retry_count() == 1,
        "shutdown recorded the wrong retry count"
    );
    assert!(
        job.retry_at_unix_ms() == Some(clock.unix_ms() + 5_000),
        "shutdown retry time changed"
    );
    assert!(
        job.last_error() == Some("launch-planning"),
        "shutdown retry category changed"
    );
    assert_eq!(worker.cancellations(), 1);
    assert_eq!(worker.shutdowns(), 1);
}
