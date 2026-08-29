use super::*;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::main_view::MainView;
use crate::server::receiver::{AttachmentRef, EmailReplyContext, InboundJob, StagedAttachment};
use crate::state::{EmailLineage, ReceiverConversationIdentity, ReceiverJobState};
use crate::tui::model::{BrainTab, Panel};
use crate::tui::receiver::attachments::{
    ReceiverAttachmentRequest, ReceiverAttachmentRuntime, ReceiverAttachmentWorkerResult,
};

use super::receiver_sync::{TestReceiverSyncRuntime, configure_receiver_sync};

#[derive(Clone)]
struct TestAttachmentRuntime {
    outcome: TestAttachmentOutcome,
    messages: Arc<Mutex<Vec<InboundJob>>>,
    completions: Arc<Mutex<VecDeque<ReceiverAttachmentWorkerResult>>>,
}

#[derive(Clone)]
enum TestAttachmentOutcome {
    Paths(Vec<std::path::PathBuf>),
    Failure,
}

impl TestAttachmentRuntime {
    fn success(path: std::path::PathBuf) -> Self {
        Self::success_paths(vec![path])
    }

    fn success_paths(paths: Vec<std::path::PathBuf>) -> Self {
        Self {
            outcome: TestAttachmentOutcome::Paths(paths),
            messages: Arc::new(Mutex::new(Vec::new())),
            completions: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    fn failure() -> Self {
        Self {
            outcome: TestAttachmentOutcome::Failure,
            messages: Arc::new(Mutex::new(Vec::new())),
            completions: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    fn messages(&self) -> Vec<InboundJob> {
        self.messages.lock().expect("attachment messages").clone()
    }
}

#[test]
fn attachment_refresh_failure_retries_without_launch_or_private_error_persistence() {
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
    app.services
        .replace_receiver_attachment_runtime(Box::new(TestAttachmentRuntime::failure()));

    app.tick_receiver();
    app.tick_receiver();

    assert!(transport.launch_specs().is_empty());
    let job = db
        .receiver_job(accepted.job_id())
        .expect("load receiver job")
        .expect("receiver job");
    assert!(
        job.state() == ReceiverJobState::Retrying,
        "attachment staging failure recorded the wrong durable state"
    );
    assert!(
        job.retry_count() == 1,
        "staging failure recorded the wrong retry count"
    );
    assert!(
        job.last_error() == Some("launch-planning"),
        "attachment staging recorded the wrong error category"
    );
    assert!(!job.last_error().unwrap_or_default().contains("credential"));
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
fn durable_dispatch_retries_when_stager_returns_a_path_outside_the_receiver_inbox() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let outside = temporary.path().join("outside.txt");
    std::fs::write(&outside, b"private attachment").expect("outside attachment");
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
    app.services
        .replace_receiver_attachment_runtime(Box::new(TestAttachmentRuntime::success(outside)));

    app.tick_receiver();
    app.tick_receiver();

    assert!(transport.launch_specs().is_empty());
    let job = db
        .receiver_job(accepted.job_id())
        .expect("load receiver job")
        .expect("receiver job");
    assert!(
        job.state() == ReceiverJobState::Retrying,
        "out-of-root attachment recorded the wrong durable state"
    );
    assert!(
        job.last_error() == Some("launch-planning"),
        "attachment download recorded the wrong error category"
    );
}

#[test]
fn durable_dispatch_retries_when_a_download_exceeds_the_attachment_size_limit() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let inbox = app.context.workspace().paths().inbox_dir();
    std::fs::create_dir_all(&inbox).expect("receiver inbox");
    let oversized = inbox.join("oversized.bin");
    let file = std::fs::File::create(&oversized).expect("oversized attachment");
    file.set_len(40 * 1024 * 1024 + 1)
        .expect("sparse oversized attachment");
    let mut inbound = receiver_job(&app, sms_actor(), Channel::Sms, "inspect the media");
    inbound.attachments = vec![AttachmentRef {
        url: "https://media.example.test/oversized".to_owned(),
        provider_id: None,
        content_type: Some("application/octet-stream".to_owned()),
        filename: Some("oversized.bin".to_owned()),
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
    app.services
        .replace_receiver_attachment_runtime(Box::new(TestAttachmentRuntime::success(oversized)));

    app.tick_receiver();
    app.tick_receiver();

    assert!(transport.launch_specs().is_empty());
    let job = db
        .receiver_job(accepted.job_id())
        .expect("load receiver job")
        .expect("receiver job");
    assert!(
        job.state() == ReceiverJobState::Retrying,
        "oversized attachment recorded the wrong durable state"
    );
    assert!(
        job.last_error() == Some("launch-planning"),
        "oversized attachment recorded the wrong error category"
    );
}

impl ReceiverAttachmentRuntime for TestAttachmentRuntime {
    fn start(&mut self, request: ReceiverAttachmentRequest) -> anyhow::Result<bool> {
        let message = request.message();
        self.messages
            .lock()
            .expect("attachment messages")
            .push(message.clone());
        let result = match &self.outcome {
            TestAttachmentOutcome::Paths(paths) => ReceiverAttachmentWorkerResult::success(
                request.stage(),
                paths
                    .iter()
                    .map(|path| StagedAttachment {
                        source: "refreshed-provider-reference".to_owned(),
                        path: Some(path.clone()),
                        error: None,
                    })
                    .collect(),
            ),
            TestAttachmentOutcome::Failure => {
                ReceiverAttachmentWorkerResult::failure(request.stage())
            }
        };
        self.completions
            .lock()
            .expect("attachment completions")
            .push_back(result);
        Ok(true)
    }

    fn poll(&mut self) -> Option<ReceiverAttachmentWorkerResult> {
        self.completions
            .lock()
            .expect("attachment completions")
            .pop_front()
    }

    fn cancel(&mut self) {}

    fn shutdown(&mut self) {}
}

#[test]
fn durable_dispatch_retries_without_staging_an_unbounded_attachment_batch() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let mut inbound = receiver_job(&app, sms_actor(), Channel::Sms, "inspect the media");
    inbound.attachments = (0..11)
        .map(|index| AttachmentRef {
            url: format!("https://media.example.test/{index}"),
            provider_id: None,
            content_type: Some("text/plain".to_owned()),
            filename: Some(format!("media-{index}.txt")),
        })
        .collect();
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
    let attachments = TestAttachmentRuntime::success_paths(Vec::new());
    app.services
        .replace_receiver_attachment_runtime(Box::new(attachments.clone()));

    app.tick_receiver();
    app.tick_receiver();

    assert!(attachments.messages().is_empty());
    assert!(transport.launch_specs().is_empty());
    let job = db
        .receiver_job(accepted.job_id())
        .expect("load receiver job")
        .expect("receiver job");
    assert!(
        job.state() == ReceiverJobState::Retrying,
        "attachment worker construction failure recorded the wrong durable state"
    );
    assert!(
        job.retry_count() == 1,
        "attachment worker recorded the wrong retry count"
    );
    assert!(
        job.last_error() == Some("launch-planning"),
        "attachment worker recorded the wrong error category"
    );
}

#[test]
fn durable_dispatch_downloads_authenticated_media_before_agent_launch() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let source_url = "https://expired.example.test/media?secret=accepted".to_owned();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    let inbox = app.context.workspace().paths().inbox_dir();
    std::fs::create_dir_all(&inbox).expect("receiver inbox");
    let local_path = inbox.join("downloaded-media.txt");
    std::fs::write(&local_path, b"private attachment").expect("attachment source");
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
    let mut inbound = receiver_job(&app, email_actor(), Channel::Email, "inspect the media");
    inbound.attachments = vec![AttachmentRef {
        url: source_url.clone(),
        provider_id: Some("stable-resend-attachment".to_owned()),
        content_type: Some("text/plain".to_owned()),
        filename: Some("accepted-media.txt".to_owned()),
    }];
    inbound.email_reply = Some(EmailReplyContext {
        provider_email_id: "stable-resend-email".to_owned(),
        subject: "Delayed attachment".to_owned(),
        message_id: Some("<thread@example.test>".to_owned()),
    });
    let identity = ReceiverConversationIdentity::email(
        app.context.workspace().id(),
        inbound.actor.user_id().clone(),
        EmailLineage::verified("thread@example.test").expect("email lineage"),
    );
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = db
        .accept_receiver_job(&inbound, &identity)
        .expect("accept durable receiver job");
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    let attachments = TestAttachmentRuntime::success(local_path.clone());
    app.services
        .replace_receiver_attachment_runtime(Box::new(attachments.clone()));

    app.tick_receiver();
    app.tick_receiver();

    let specifications = transport.launch_specs();
    assert_eq!(specifications.len(), 1);
    assert!(!specifications[0].command.contains(&source_url));
    assert!(
        specifications[0]
            .command
            .contains(&local_path.display().to_string())
    );
    let messages = attachments.messages();
    assert!(
        messages.len() == 1 && inbound_job_proof(&messages[0]) == inbound_job_proof(&inbound),
        "attachment runtime did not receive the exact authenticated message"
    );
    assert!(
        db.receiver_job(accepted.job_id())
            .expect("load receiver job")
            .expect("receiver job")
            .state()
            == ReceiverJobState::Launched,
        "downloaded attachment recorded the wrong durable state"
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
fn receiver_freshness_finishes_before_attachment_refresh_and_background_launch() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    configure_receiver_sync(&app);
    let sync = TestReceiverSyncRuntime::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(sync.clone()));
    app.shell.show_main_view(MainView::BrainSearch);
    let before = (
        app.shell.main_view(),
        app.effective_brain_tab(),
        app.shell.focus(),
    );
    let inbox = app.context.workspace().paths().inbox_dir();
    std::fs::create_dir_all(&inbox).expect("receiver inbox");
    let local_path = inbox.join("fresh-media.txt");
    std::fs::write(&local_path, b"private attachment").expect("attachment source");
    let mut inbound = receiver_job(&app, sms_actor(), Channel::Sms, "inspect after sync");
    inbound.attachments = vec![AttachmentRef {
        url: "https://media.example.test/accepted".to_owned(),
        provider_id: None,
        content_type: Some("text/plain".to_owned()),
        filename: Some("accepted.txt".to_owned()),
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
    let attachments = TestAttachmentRuntime::success(local_path.clone());
    app.services
        .replace_receiver_attachment_runtime(Box::new(attachments.clone()));

    app.tick_receiver();

    assert!(attachments.messages().is_empty());
    assert!(transport.launch_specs().is_empty());
    assert!(
        db.receiver_job(accepted.job_id())
            .expect("load receiver job")
            .expect("receiver job")
            .state()
            == ReceiverJobState::Claimed,
        "pending attachment download recorded the wrong durable state"
    );
    assert_eq!(
        (
            app.shell.main_view(),
            app.effective_brain_tab(),
            app.shell.focus(),
        ),
        before
    );

    sync.finish_pull();
    app.tick_receiver();

    let messages = attachments.messages();
    assert!(
        messages.len() == 1 && inbound_job_proof(&messages[0]) == inbound_job_proof(&inbound),
        "attachment runtime did not retain the exact authenticated message"
    );
    assert!(transport.launch_specs().is_empty());

    app.tick_receiver();

    let messages = attachments.messages();
    assert!(
        messages.len() == 1 && inbound_job_proof(&messages[0]) == inbound_job_proof(&inbound),
        "attachment runtime changed the authenticated message"
    );
    let specifications = transport.launch_specs();
    assert_eq!(specifications.len(), 1);
    assert!(
        specifications[0]
            .command
            .contains(&local_path.display().to_string())
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

fn inbound_job_proof(message: &InboundJob) -> (usize, [u8; 32], usize, [u8; 32]) {
    use sha2::Digest as _;

    let serialized = serde_json::to_vec(message).expect("serialize inbound proof");
    (
        serialized.len(),
        sha2::Sha256::digest(&serialized).into(),
        message.response_sender.len(),
        sha2::Sha256::digest(message.response_sender.as_bytes()).into(),
    )
}
