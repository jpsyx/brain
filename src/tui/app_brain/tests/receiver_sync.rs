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

fn configure_receiver_sync(app: &App) {
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
    let workspace_id = app.command_context.workspace.id();
    enqueue_receiver_job(
        &mut app,
        InboundJob {
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
        },
    );

    app.tick_receiver();

    assert_eq!(
        app.receiver.pending_count(),
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
    app.receiver_sync_runtime = Box::new(runtime.clone());

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
    assert_eq!(app.last_seen_downstream_id, None);
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

#[test]
fn a_successful_downstream_sync_reloads_tasks_without_a_manual_refresh() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    let runtime = TestReceiverSyncRuntime::new();
    app.sync_status_next_poll = crate::tui::ReceiverSyncRuntime::monotonic_now(&runtime);
    app.receiver_sync_runtime = Box::new(runtime);
    std::fs::write(
        &app.csv_path,
        "task_uuid,task_id,task_name,task_type,status,waiting_since,priority,due_date,hard_deadline,start_date,assigned_to,see_also,notes,project,energy_level,context,estimated_duration,blocked_by,defer_count,created_date,completed_date,last_touched,linear_issue,system_key,backlogged_date\n\
         55dc97d4-daa0-4e9c-b36c-78550f153f58,T900,Review synced task,code,backlog,,p2,2026-08-30,true,,test-user,,,,,,,,0,2026-08-20,,2026-08-20,,,\n",
    )
    .expect("write downstream task");
    assert_eq!(
        crate::tasks::task::load_tasks(&app.csv_path)
            .expect("load downstream task fixture")
            .len(),
        1,
        "the fixture must be a valid task before exercising the refresh"
    );

    app.tick_sync_status();

    assert_eq!(app.last_seen_downstream_id, Some(4));
    assert!(
        app.tasks
            .source_rows()
            .0
            .iter()
            .any(|task| task.name == "Review synced task"),
        "the live TUI stayed stale after a successful downstream sync"
    );
}

#[test]
fn receiver_completion_immediately_publishes_agent_created_changes() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    configure_receiver_sync(&app);
    let runtime = TestReceiverSyncRuntime::new();
    app.receiver_sync_runtime = Box::new(runtime.clone());
    let actor = sms_actor();
    app.session_actor = Some(actor.clone());
    let job = receiver_job(&app, actor.clone(), Channel::Sms, "capture this task");
    begin_receiver_turn(
        &mut app,
        &job,
        "receiver-push-session",
        std::time::Instant::now(),
    );
    let response_path = app
        .command_context
        .workspace
        .paths()
        .responses_dir()
        .join("receiver-push-session.json");
    std::fs::create_dir_all(response_path.parent().expect("response directory"))
        .expect("create response directory");
    std::fs::write(
        response_path,
        serde_json::json!({
            "actor_id": actor.user_id().as_str(),
            "channel": "sms",
            "message": "Task captured."
        })
        .to_string(),
    )
    .expect("write receiver completion");

    app.tick_receiver();

    assert_eq!(
        runtime.state.lock().unwrap().launches,
        [(
            app.command_context.workspace.id(),
            crate::sync::args::Direction::Push,
        )],
        "receiver completion must publish without relying only on the watcher"
    );
}
