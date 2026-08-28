use super::receiver_durable_support::{accept_email_job, publish_valid_completion};
use super::*;

use crate::state::{ReceiverConversationIdentity, ReceiverJobState};

#[derive(Clone)]
pub(super) struct TestReceiverSyncRuntime {
    state: Arc<Mutex<TestReceiverSyncState>>,
}

struct TestReceiverSyncState {
    monotonic: std::time::Instant,
    utc: chrono::DateTime<chrono::Utc>,
    live: Option<crate::sync::current::CurrentState>,
    journal_id: Option<i64>,
    last_downstream: Option<String>,
    launches: Vec<(WorkspaceId, crate::sync::args::Direction)>,
    live_reads: usize,
}

impl TestReceiverSyncRuntime {
    pub(super) fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(TestReceiverSyncState {
                monotonic: std::time::Instant::now(),
                utc: chrono::TimeZone::with_ymd_and_hms(&chrono::Utc, 2026, 8, 5, 12, 0, 0)
                    .unwrap(),
                live: None,
                journal_id: Some(4),
                last_downstream: None,
                launches: Vec::new(),
                live_reads: 0,
            })),
        }
    }

    fn advance(&self, duration: std::time::Duration) {
        let mut state = self.state.lock().unwrap();
        state.monotonic += duration;
        state.utc += chrono::TimeDelta::from_std(duration).unwrap();
    }

    fn unix_ms(&self) -> u64 {
        u64::try_from(self.state.lock().unwrap().utc.timestamp_millis()).unwrap()
    }

    pub(super) fn finish_pull(&self) {
        let mut state = self.state.lock().unwrap();
        state.journal_id = Some(state.journal_id.unwrap_or_default() + 1);
        drop(state);
        self.advance(std::time::Duration::from_millis(250));
    }
}

impl crate::tui::app_sync::ReceiverSyncRuntime for TestReceiverSyncRuntime {
    fn monotonic_now(&self) -> std::time::Instant {
        self.state.lock().unwrap().monotonic
    }

    fn utc_now(&self) -> chrono::DateTime<chrono::Utc> {
        self.state.lock().unwrap().utc
    }

    fn live_sync_state(
        &self,
        _paths: &crate::workspace::WorkspacePaths,
    ) -> Option<crate::sync::current::CurrentState> {
        let mut state = self.state.lock().unwrap();
        state.live_reads += 1;
        state.live.clone()
    }

    fn latest_successful_downstream_id(
        &self,
        _paths: &crate::workspace::WorkspacePaths,
    ) -> Option<i64> {
        self.state.lock().unwrap().journal_id
    }

    fn latest_downstream_completion(
        &self,
        _paths: &crate::workspace::WorkspacePaths,
    ) -> Option<String> {
        self.state.lock().unwrap().last_downstream.clone()
    }

    fn spawn_detached_sync(
        &self,
        workspace: &WorkspaceContext,
        direction: crate::sync::args::Direction,
    ) -> Option<u32> {
        let mut state = self.state.lock().unwrap();
        state.launches.push((workspace.id(), direction));
        u32::try_from(state.launches.len()).ok()
    }
}

pub(super) fn configure_receiver_sync(app: &App) {
    let selected_name = app.context.workspace().name().clone();
    let mut registry =
        RegistryStore::load_from(app.context.command().registry_store.path()).unwrap();
    registry
        .workspaces
        .get_mut(&selected_name)
        .unwrap()
        .env
        .insert(
            "sync".to_owned(),
            serde_json::json!({"enabled": true, "b2_bucket": "test-bucket"}),
        );
    app.context
        .command()
        .registry_store
        .replace(&registry)
        .unwrap();
}

#[test]
fn durable_receiver_claim_stays_owned_while_workspace_freshness_is_pending() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    configure_receiver_sync(&app);
    let runtime = TestReceiverSyncRuntime::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(runtime.clone()));
    let actor = sms_actor();
    let workspace_id = app.context.workspace().id();
    let inbound = InboundJob {
        job_id: uuid::Uuid::new_v4(),
        workspace_id,
        actor,
        channel: Channel::Sms,
        prompt: "wait for the remote brain".to_owned(),
        authenticated_sender: "+15551234567".to_owned(),
        attachments: Vec::new(),
        received_at_unix_ms: 1,
        provider_id: Some("provider-message-1".to_owned()),
        thread_participants: vec!["+15551234567".to_owned()],
        response_email: None,
        allowed_response_recipients: Vec::new(),
        email_reply: None,
    };
    let identity = ReceiverConversationIdentity::sms(
        app.context.workspace().id(),
        inbound.actor.user_id().clone(),
    );
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = db
        .accept_receiver_job(&inbound, &identity)
        .expect("accept durable receiver job");
    let receiver_recording = TransportRecording::default();
    app.brain
        .replace_receiver_transport(receiver_recording.transport());

    app.tick_receiver();

    assert_eq!(
        db.receiver_job(accepted.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Claimed,
        "freshness must run before launch preparation"
    );
    assert!(app.brain.receiver_run_observations().is_empty());
    assert!(receiver_recording.launch_specs().is_empty());
    assert_eq!(
        runtime.state.lock().unwrap().launches,
        [(
            app.context.workspace().id(),
            crate::sync::args::Direction::Pull,
        )]
    );

    runtime.advance(std::time::Duration::from_secs(20));
    app.tick_receiver();
    runtime.advance(std::time::Duration::from_secs(15));
    let now = runtime.unix_ms();
    assert!(
        db.claim_next_receiver_run("competing-owner", now, now + 30_000)
            .expect("competing claim")
            .is_none(),
        "a pending freshness pull must not let the exact durable claim expire"
    );
    assert!(app.brain.receiver_run_observations().is_empty());

    runtime.state.lock().unwrap().journal_id = Some(5);
    runtime.advance(std::time::Duration::from_millis(250));
    app.tick_receiver();

    assert_eq!(
        app.brain.receiver_run_observations().len(),
        1,
        "journal completion should launch the claimed job; durable job is {:?}",
        db.receiver_job(accepted.job_id()).unwrap().unwrap()
    );
    assert_eq!(
        app.brain.receiver_run_observations()[0].job_id,
        accepted.job_id()
    );
    assert_eq!(receiver_recording.launch_specs().len(), 1);
}

#[test]
fn pending_freshness_claim_remains_managed_after_receiver_intent_is_disabled() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    configure_receiver_sync(&app);
    let runtime = TestReceiverSyncRuntime::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(runtime.clone()));
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "finish freshness despite disable", 100);
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());

    app.tick_receiver();
    assert_eq!(
        db.receiver_job(accepted.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Claimed
    );

    app.receiver.record_intent(false);
    runtime.advance(std::time::Duration::from_secs(20));
    app.tick_receiver();
    runtime.advance(std::time::Duration::from_secs(15));
    let now = runtime.unix_ms();
    assert!(
        db.claim_next_receiver_run("competing-owner", now, now + 30_000)
            .expect("competing claim")
            .is_none(),
        "disabling intent must not abandon a claim waiting on freshness"
    );

    runtime.state.lock().unwrap().journal_id = Some(5);
    runtime.advance(std::time::Duration::from_millis(250));
    app.tick_receiver();

    assert!(app.brain.receiver_run_observations().is_empty());
    assert_eq!(
        db.receiver_job(accepted.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Claimed
    );
    assert!(transport.launch_specs().is_empty());

    app.receiver.record_intent(true);
    app.tick_receiver();

    assert_eq!(app.brain.receiver_run_observations().len(), 1);
    assert_eq!(
        app.brain.receiver_run_observations()[0].job_id,
        accepted.job_id()
    );
    assert_eq!(transport.launch_specs().len(), 1);
}

#[test]
fn receiver_freshness_gate_opens_only_after_the_injected_journal_advances() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    configure_receiver_sync(&app);
    let runtime = TestReceiverSyncRuntime::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(runtime.clone()));

    assert_eq!(
        app.execute_receiver_sync_freshness_effect(),
        crate::tui::receiver::ReceiverEffectOutcome::FreshnessPending
    );
    assert_eq!(
        app.execute_receiver_sync_freshness_effect(),
        crate::tui::receiver::ReceiverEffectOutcome::FreshnessPending,
        "unchanged journal must remain gated"
    );
    runtime.state.lock().unwrap().journal_id = Some(5);
    runtime.advance(std::time::Duration::from_millis(250));

    assert_eq!(
        app.execute_receiver_sync_freshness_effect(),
        crate::tui::receiver::ReceiverEffectOutcome::Completed
    );
    assert_eq!(runtime.state.lock().unwrap().launches.len(), 1);
}

#[test]
fn receiver_pull_retries_and_falls_back_after_three_clock_driven_grace_periods() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    configure_receiver_sync(&app);
    let runtime = TestReceiverSyncRuntime::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(runtime.clone()));

    assert_eq!(
        app.execute_receiver_sync_freshness_effect(),
        crate::tui::receiver::ReceiverEffectOutcome::FreshnessPending,
        "attempt one launches"
    );
    runtime.advance(std::time::Duration::from_secs(4));
    assert_eq!(
        app.execute_receiver_sync_freshness_effect(),
        crate::tui::receiver::ReceiverEffectOutcome::FreshnessPending,
        "grace period suppresses retry"
    );
    assert_eq!(runtime.state.lock().unwrap().launches.len(), 1);
    runtime.advance(std::time::Duration::from_secs(1));
    assert_eq!(
        app.execute_receiver_sync_freshness_effect(),
        crate::tui::receiver::ReceiverEffectOutcome::FreshnessPending,
        "attempt two launches at five seconds"
    );
    runtime.advance(std::time::Duration::from_secs(5));
    assert_eq!(
        app.execute_receiver_sync_freshness_effect(),
        crate::tui::receiver::ReceiverEffectOutcome::FreshnessPending,
        "attempt three launches at ten seconds"
    );
    runtime.advance(std::time::Duration::from_secs(5));

    assert_eq!(
        app.execute_receiver_sync_freshness_effect(),
        crate::tui::receiver::ReceiverEffectOutcome::Completed,
        "third failed start falls back locally"
    );
    assert_eq!(runtime.state.lock().unwrap().launches.len(), 3);
}

#[test]
fn sync_status_poll_uses_the_injected_clock_and_bounded_interval() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    assert_eq!(app.status.last_seen_downstream_id(), None);
    let runtime = TestReceiverSyncRuntime::new();
    app.status
        .set_sync_poll_deadline(crate::tui::app_sync::ReceiverSyncRuntime::monotonic_now(
            &runtime,
        ));
    app.services
        .replace_receiver_sync_runtime(Box::new(runtime.clone()));

    app.tick_sync_status();
    app.tick_sync_status();
    assert_eq!(runtime.state.lock().unwrap().live_reads, 1);
    runtime.advance(std::time::Duration::from_millis(250));
    app.tick_sync_status();

    assert_eq!(runtime.state.lock().unwrap().live_reads, 2);
}

#[test]
fn a_successful_downstream_sync_reloads_tasks_without_a_manual_refresh() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    let runtime = TestReceiverSyncRuntime::new();
    app.status
        .set_sync_poll_deadline(crate::tui::app_sync::ReceiverSyncRuntime::monotonic_now(
            &runtime,
        ));
    app.services
        .replace_receiver_sync_runtime(Box::new(runtime));
    std::fs::write(
        app.context.tasks_csv_path(),
        "task_uuid,task_id,task_name,task_type,status,waiting_since,priority,due_date,hard_deadline,start_date,assigned_to,see_also,notes,project,energy_level,context,estimated_duration,blocked_by,defer_count,created_date,completed_date,last_touched,linear_issue,system_key,backlogged_date\n\
         55dc97d4-daa0-4e9c-b36c-78550f153f58,T900,Review synced task,code,backlog,,p2,2026-08-30,true,,test-user,,,,,,,,0,2026-08-20,,2026-08-20,,,\n",
    )
    .expect("write downstream task");
    assert_eq!(
        crate::tasks::task::load_tasks(app.context.tasks_csv_path())
            .expect("load downstream task fixture")
            .len(),
        1,
        "the fixture must be a valid task before exercising the refresh"
    );

    app.tick_sync_status();

    assert_eq!(app.status.last_seen_downstream_id(), Some(4));
    assert!(
        app.tasks.contains_task_named("Review synced task"),
        "the live TUI stayed stale after a successful downstream sync"
    );
}

#[test]
fn receiver_completion_immediately_publishes_agent_created_changes() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    configure_receiver_sync(&app);
    let runtime = TestReceiverSyncRuntime::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(runtime.clone()));
    let actor = sms_actor();
    let inbound = receiver_job(&app, actor.clone(), Channel::Sms, "capture this task");
    let identity =
        ReceiverConversationIdentity::sms(app.context.workspace().id(), actor.user_id().clone());
    let db = Db::open(app.context.workspace()).expect("state DB");
    db.accept_receiver_job(&inbound, &identity)
        .expect("accept durable receiver job");
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    app.tick_receiver();
    runtime.state.lock().unwrap().journal_id = Some(5);
    runtime.advance(std::time::Duration::from_millis(250));
    app.tick_receiver();
    runtime.state.lock().unwrap().launches.clear();
    publish_valid_completion(&app, "Task captured.");

    app.tick_receiver();

    assert_eq!(
        runtime.state.lock().unwrap().launches,
        [(
            app.context.workspace().id(),
            crate::sync::args::Direction::Push,
        )],
        "receiver completion must publish without relying only on the watcher"
    );
}
