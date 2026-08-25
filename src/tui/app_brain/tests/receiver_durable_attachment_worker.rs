use super::*;

use crate::main_view::MainView;
use crate::server::receiver::{AttachmentRef, StagedAttachment};
use crate::state::{ReceiverConversationIdentity, ReceiverJobState};
use crate::tui::model::{BrainTab, Panel};

use super::receiver_attachment_worker_support::ControlledAttachmentWorker;
use super::receiver_durable_support::ReceiverClock;

#[test]
fn attachment_staging_starts_and_the_tick_returns_without_launch_or_focus_mutation() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    app.shell.show_main_view(MainView::BrainSearch);
    let before = (
        app.shell.main_view(),
        app.effective_brain_tab(),
        app.shell.focus(),
    );
    assert_eq!(
        before,
        (MainView::BrainSearch, BrainTab::Main, Panel::Tasks)
    );
    let mut inbound = receiver_job(&app, sms_actor(), Channel::Sms, "inspect the media");
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
        .expect("accept durable receiver job");
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    let worker = ControlledAttachmentWorker::default();
    app.services
        .replace_receiver_attachment_runtime(Box::new(worker.clone()));

    app.tick_receiver();

    assert_eq!(worker.starts(), 1);
    assert!(transport.launch_specs().is_empty());
    assert_eq!(
        db.receiver_job(accepted.job_id())
            .expect("load receiver job")
            .expect("receiver job")
            .state(),
        ReceiverJobState::Claimed
    );
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
fn pending_attachment_staging_renews_the_exact_durable_claim() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let mut inbound = receiver_job(&app, sms_actor(), Channel::Sms, "inspect the media");
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
    db.accept_receiver_job(&inbound, &identity)
        .expect("accept durable receiver job");
    let worker = ControlledAttachmentWorker::default();
    app.services
        .replace_receiver_attachment_runtime(Box::new(worker.clone()));

    app.tick_receiver();
    clock.advance(std::time::Duration::from_secs(20));
    app.tick_receiver();
    clock.advance(std::time::Duration::from_secs(15));
    let now = clock.unix_ms();

    assert_eq!(worker.starts(), 1);
    assert!(
        db.claim_next_receiver_run("competing-owner", now, now + 30_000)
            .expect("competing claim")
            .is_none(),
        "pending attachment staging must renew before its original claim expires"
    );
}

#[test]
fn completed_staging_uses_fresh_time_and_discards_an_expired_claim() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let inbox = app.context.workspace().paths().inbox_dir();
    std::fs::create_dir_all(&inbox).expect("receiver inbox");
    let local_path = inbox.join("expired-owner.txt");
    std::fs::write(&local_path, b"private attachment").expect("staged attachment");
    let mut inbound = receiver_job(&app, sms_actor(), Channel::Sms, "inspect the media");
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
        .expect("accept durable receiver job");
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    let worker = ControlledAttachmentWorker::default();
    app.services
        .replace_receiver_attachment_runtime(Box::new(worker.clone()));

    app.tick_receiver();
    worker.complete(
        worker.stage(0),
        vec![StagedAttachment {
            source: "refreshed-provider-reference".to_owned(),
            path: Some(local_path.clone()),
            error: None,
        }],
    );
    clock.advance(std::time::Duration::from_secs(29));
    worker.advance_on_next_poll(clock, std::time::Duration::from_secs(31));

    app.tick_receiver();

    assert!(transport.launch_specs().is_empty());
    assert!(
        !local_path.exists(),
        "discarded staged files must be cleaned"
    );
    let job = db
        .receiver_job(accepted.job_id())
        .expect("load receiver job")
        .expect("receiver job");
    assert_eq!(job.state(), ReceiverJobState::Claimed);
    assert_eq!(job.retry_count(), 0);
}

#[test]
fn staging_failure_records_retry_from_the_post_result_clock() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let mut inbound = receiver_job(&app, sms_actor(), Channel::Sms, "inspect the media");
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
        .expect("accept durable receiver job");
    let worker = ControlledAttachmentWorker::default();
    app.services
        .replace_receiver_attachment_runtime(Box::new(worker.clone()));

    app.tick_receiver();
    worker.fail(worker.stage(0));
    worker.advance_on_next_poll(clock.clone(), std::time::Duration::from_secs(7));
    app.tick_receiver();

    let job = db
        .receiver_job(accepted.job_id())
        .expect("load receiver job")
        .expect("receiver job");
    assert_eq!(job.state(), ReceiverJobState::Retrying);
    assert_eq!(job.retry_at_unix_ms(), Some(clock.unix_ms() + 5_000));
}

#[test]
fn disabling_pending_staging_cancels_and_app_shutdown_stops_the_worker() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let mut inbound = receiver_job(&app, sms_actor(), Channel::Sms, "inspect the media");
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
        .expect("accept durable receiver job");
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    let worker = ControlledAttachmentWorker::default();
    app.services
        .replace_receiver_attachment_runtime(Box::new(worker.clone()));

    app.tick_receiver();
    app.receiver.record_intent(false);
    app.tick_receiver();

    assert_eq!(worker.cancellations(), 1);
    assert!(transport.launch_specs().is_empty());
    assert_eq!(
        db.receiver_job(accepted.job_id())
            .expect("load receiver job")
            .expect("receiver job")
            .state(),
        ReceiverJobState::Claimed
    );

    app.services.shutdown_receiver_attachments();
    assert_eq!(worker.shutdowns(), 1);
    drop(app);
    assert_eq!(worker.shutdowns(), 2);
}

#[test]
fn a_later_fifo_job_cannot_overtake_pending_attachment_staging() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let mut first = receiver_job(&app, sms_actor(), Channel::Sms, "inspect the media");
    first.attachments = vec![AttachmentRef {
        url: "https://media.example.test/private".to_owned(),
        provider_id: None,
        content_type: Some("text/plain".to_owned()),
        filename: Some("private.txt".to_owned()),
    }];
    let first_identity = ReceiverConversationIdentity::sms(
        app.context.workspace().id(),
        first.actor.user_id().clone(),
    );
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted_first = db
        .accept_receiver_job(&first, &first_identity)
        .expect("accept first durable receiver job");
    let mut second = receiver_job(&app, email_actor(), Channel::Email, "later work");
    second.received_at_unix_ms = 2;
    let second_identity = ReceiverConversationIdentity::email(
        app.context.workspace().id(),
        second.actor.user_id().clone(),
        crate::state::EmailLineage::verified("thread@example.test").expect("email lineage"),
    );
    let accepted_second = db
        .accept_receiver_job(&second, &second_identity)
        .expect("accept second durable receiver job");
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    let worker = ControlledAttachmentWorker::default();
    app.services
        .replace_receiver_attachment_runtime(Box::new(worker.clone()));

    app.tick_receiver();
    app.tick_receiver();

    assert_eq!(worker.starts(), 1);
    assert!(transport.launch_specs().is_empty());
    assert_eq!(
        db.receiver_job(accepted_first.job_id())
            .expect("load first job")
            .expect("first job")
            .state(),
        ReceiverJobState::Claimed
    );
    assert_eq!(
        db.receiver_job(accepted_second.job_id())
            .expect("load second job")
            .expect("second job")
            .state(),
        ReceiverJobState::Queued
    );
}

#[test]
fn a_late_prior_generation_result_cannot_attach_to_the_restarted_stage() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let inbox = app.context.workspace().paths().inbox_dir();
    std::fs::create_dir_all(&inbox).expect("receiver inbox");
    let stale_path = inbox.join("stale-generation.txt");
    let current_path = inbox.join("current-generation.txt");
    std::fs::write(&stale_path, b"stale attachment").expect("stale attachment");
    std::fs::write(&current_path, b"current attachment").expect("current attachment");
    let mut inbound = receiver_job(&app, sms_actor(), Channel::Sms, "inspect the media");
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
        .expect("accept durable receiver job");
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    let worker = ControlledAttachmentWorker::default();
    app.services
        .replace_receiver_attachment_runtime(Box::new(worker.clone()));

    app.tick_receiver();
    let stale_stage = worker.stage(0);
    assert_eq!(stale_stage.job_id(), accepted.job_id());
    app.receiver.record_intent(false);
    app.tick_receiver();
    app.receiver.record_intent(true);
    app.tick_receiver();
    let current_stage = worker.stage(1);
    worker.complete(
        stale_stage,
        vec![StagedAttachment {
            source: "stale-provider-reference".to_owned(),
            path: Some(stale_path.clone()),
            error: None,
        }],
    );
    worker.complete(
        current_stage,
        vec![StagedAttachment {
            source: "current-provider-reference".to_owned(),
            path: Some(current_path.clone()),
            error: None,
        }],
    );

    app.tick_receiver();

    assert!(transport.launch_specs().is_empty());
    assert!(!stale_path.exists());
    assert!(current_path.exists());

    app.tick_receiver();

    let launches = transport.launch_specs();
    assert_eq!(launches.len(), 1);
    assert!(
        launches[0]
            .command
            .contains(&current_path.display().to_string())
    );
    assert!(!launches[0].command.contains("stale-generation.txt"));
}
