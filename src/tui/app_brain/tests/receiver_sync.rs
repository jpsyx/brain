use super::*;

#[derive(Clone)]
struct TestReceiverSyncRuntime {
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
    fn new() -> Self {
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
}

impl crate::tui::ReceiverSyncRuntime for TestReceiverSyncRuntime {
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

    fn latest_journal_id(&self, _paths: &crate::workspace::WorkspacePaths) -> Option<i64> {
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

fn configure_receiver_sync(app: &App<'_>) {
    let selected_name = app.command_context.workspace.name().clone();
    app.command_context
        .registry_store
        .replace(&crate::workspace::MachineRegistry {
            schema_version: crate::workspace::REGISTRY_SCHEMA_VERSION,
            default_workspace: selected_name.clone(),
            workspaces: std::collections::BTreeMap::from([(
                selected_name.clone(),
                crate::workspace::WorkspaceRecord {
                    workspace_id: app.command_context.workspace.id(),
                    root: app.command_context.workspace.root().to_path_buf(),
                    aliases: std::collections::BTreeSet::new(),
                    local_user_id: app.command_context.workspace.local_user_id().to_owned(),
                    receiver_enabled: false,
                    env: serde_json::Map::new(),
                },
            )]),
            env: serde_json::Map::new(),
        })
        .unwrap();
    let mut registry = RegistryStore::load_from(app.command_context.registry_store.path()).unwrap();
    registry
        .workspaces
        .get_mut(&selected_name)
        .unwrap()
        .env
        .insert(
            "sync".to_owned(),
            serde_json::json!({"enabled": true, "b2_bucket": "test-bucket"}),
        );
    app.command_context
        .registry_store
        .replace(&registry)
        .unwrap();
}

#[test]
fn receiver_job_consumption_waits_for_this_workspace_freshness_pull() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    configure_receiver_sync(&app);
    let runtime = TestReceiverSyncRuntime::new();
    app.receiver_sync_runtime = Box::new(runtime.clone());
    let actor = app.interactive_actor.clone();
    app.receiver_queue.push(InboundJob {
        job_id: uuid::Uuid::new_v4(),
        workspace_id: app.command_context.workspace.id(),
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
    });

    app.tick_receiver();

    assert_eq!(
        app.receiver_queue.len(),
        1,
        "queued work must not dispatch early"
    );
    assert_eq!(
        runtime.state.lock().unwrap().launches,
        [(
            app.command_context.workspace.id(),
            crate::sync::args::Direction::Pull,
        )]
    );
}

#[test]
fn receiver_freshness_gate_opens_only_after_the_injected_journal_advances() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    configure_receiver_sync(&app);
    let runtime = TestReceiverSyncRuntime::new();
    app.receiver_sync_runtime = Box::new(runtime.clone());

    assert!(!app.receiver_sync_ready());
    assert!(
        !app.receiver_sync_ready(),
        "unchanged journal must remain gated"
    );
    runtime.state.lock().unwrap().journal_id = Some(5);
    runtime.advance(std::time::Duration::from_millis(250));

    assert!(app.receiver_sync_ready());
    assert_eq!(runtime.state.lock().unwrap().launches.len(), 1);
}

#[test]
fn receiver_pull_retries_and_falls_back_after_three_clock_driven_grace_periods() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    configure_receiver_sync(&app);
    let runtime = TestReceiverSyncRuntime::new();
    app.receiver_sync_runtime = Box::new(runtime.clone());

    assert!(!app.receiver_sync_ready(), "attempt one launches");
    runtime.advance(std::time::Duration::from_secs(4));
    assert!(!app.receiver_sync_ready(), "grace period suppresses retry");
    assert_eq!(runtime.state.lock().unwrap().launches.len(), 1);
    runtime.advance(std::time::Duration::from_secs(1));
    assert!(
        !app.receiver_sync_ready(),
        "attempt two launches at five seconds"
    );
    runtime.advance(std::time::Duration::from_secs(5));
    assert!(
        !app.receiver_sync_ready(),
        "attempt three launches at ten seconds"
    );
    runtime.advance(std::time::Duration::from_secs(5));

    assert!(
        app.receiver_sync_ready(),
        "third failed start falls back locally"
    );
    assert_eq!(runtime.state.lock().unwrap().launches.len(), 3);
}

#[test]
fn sync_status_poll_uses_the_injected_clock_and_bounded_interval() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    let runtime = TestReceiverSyncRuntime::new();
    app.sync_status_next_poll = crate::tui::ReceiverSyncRuntime::monotonic_now(&runtime);
    app.receiver_sync_runtime = Box::new(runtime.clone());

    app.tick_sync_status();
    app.tick_sync_status();
    assert_eq!(runtime.state.lock().unwrap().live_reads, 1);
    runtime.advance(std::time::Duration::from_millis(250));
    app.tick_sync_status();

    assert_eq!(runtime.state.lock().unwrap().live_reads, 2);
}
