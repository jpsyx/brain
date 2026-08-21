use super::*;

/// A dispatched turn that never signals completion used to pin the panel
/// forever: the inactivity lease only expires once nothing is in flight, so
/// every message behind it was answered with the processing notice and nothing
/// else. The stuck turn is abandoned so the queue drains.
#[test]
fn a_stuck_remote_turn_is_abandoned_so_the_messages_behind_it_still_get_answered() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    let actor = app.interactive_actor.clone();
    let live = live_panel(app.command_context.workspace.root());
    app.brain = Some(panel_controller(&app, live));
    let relaunch = LaunchRecording::default();
    app.brain_transport_override = Some(Box::new(LaunchRecordingTransport {
        recording: relaunch,
        alive: true,
    }));
    app.session_actor = Some(actor.clone());
    // In flight for longer than any answer is allowed to take, with no
    // completion artifact ever written.
    let started = std::time::Instant::now()
        .checked_sub(crate::tui::receiver_state::REMOTE_TURN_TIMEOUT)
        .expect("a deadline already in the past");
    let wedged_job = receiver_job(&app, actor.clone(), Channel::Sms, "wedged request");
    begin_receiver_turn(&mut app, &wedged_job, "wedged-session", started);
    let quiet = std::time::Instant::now()
        .checked_sub(crate::tui::receiver_state::ACTIVE_WORK_IDLE)
        .expect("a panel that went quiet");
    app.receiver.note_panel_sample(quiet, Some(0));
    app.receiver
        .note_panel_sample(std::time::Instant::now(), None);
    app.brain_turn_active = true;
    let workspace_id = app.command_context.workspace.id();
    enqueue_receiver_job(
        &mut app,
        InboundJob {
            job_id: uuid::Uuid::new_v4(),
            workspace_id,
            actor,
            channel: Channel::Sms,
            prompt: "when did I last pick up lexapro?".to_owned(),
            authenticated_sender: "+15551234567".to_owned(),
            attachments: Vec::new(),
            received_at_unix_ms: 1,
            provider_id: Some("provider-message-2".to_owned()),
            thread_participants: vec!["+15551234567".to_owned()],
            response_email: None,
            allowed_response_recipients: Vec::new(),
            email_reply: None,
        },
    );

    app.tick_receiver();

    assert!(
        !app.brain_turn_active || app.receiver.remote_turn_in_flight(),
        "the wedged turn must not still be pinning the panel"
    );
    assert!(
        app.receiver.remote_started_at().is_none_or(
            |started| started.elapsed() < crate::tui::receiver_state::REMOTE_TURN_TIMEOUT
        ),
        "the abandoned turn's deadline must be cleared or replaced by a fresh dispatch"
    );

    // The queue must make progress rather than waiting on the wedged turn.
    app.tick_receiver();
    assert!(
        !app.receiver.has_pending_work(),
        "the message behind the wedged turn must still get dispatched"
    );
}

/// The exact bytes a warm-panel reuse puts on the PTY, captured through a real
/// `PtyPane`. This is the delivery that silently failed: the prompt appeared in
/// the composer and no turn ever started, so the message got no reply.
#[test]
fn warm_panel_reuse_delivers_a_closed_paste_and_a_submit_key_to_the_real_pty() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    let actor = app.interactive_actor.clone();
    let root = app.command_context.workspace.root().to_path_buf();
    let captured = root.join("captured.bin");
    let panel = crate::pty_pane::PtyPane::spawn_shell_command_with_env(
        &format!("stty raw -echo; printf READY; cat > {}", captured.display()),
        &[],
        &root,
        24,
        80,
    )
    .expect("spawn capture panel");
    app.brain = Some(panel_controller(&app, panel));
    assert!(
        wait_for_panel_contents(app.brain.as_ref().expect("panel"), "READY"),
        "capture panel never became ready"
    );
    app.session_actor = Some(actor.clone());
    let warm_job = receiver_job(&app, actor.clone(), Channel::Sms, "previous request");
    warm_receiver_session(
        &mut app,
        &warm_job,
        "warm-session",
        std::time::Instant::now(),
    );
    let workspace_id = app.command_context.workspace.id();
    enqueue_receiver_job(
        &mut app,
        InboundJob {
            job_id: uuid::Uuid::new_v4(),
            workspace_id,
            actor,
            channel: Channel::Sms,
            prompt: "How many projects do we have open?".to_owned(),
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
    assert!(
        !app.receiver.has_pending_work(),
        "message was not dispatched"
    );
    assert!(
        app.receiver.has_scheduled_probe(),
        "a dispatched message must be sampled, or an unsubmitted prompt leaves no evidence"
    );

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let bytes = loop {
        let bytes = std::fs::read(&captured).unwrap_or_default();
        if bytes.ends_with(b"\r") || std::time::Instant::now() >= deadline {
            break bytes;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let rendered = String::from_utf8_lossy(&bytes).into_owned();

    assert!(
        bytes.starts_with(b"\x1b[200~"),
        "delivery must open a bracketed paste, got: {rendered:?}"
    );
    assert!(
        bytes.ends_with(b"\x1b[201~\r"),
        "delivery must close the paste and then submit, got tail: {:?}",
        String::from_utf8_lossy(&bytes[bytes.len().saturating_sub(24)..])
    );
    assert!(
        rendered.contains("How many projects do we have open?"),
        "the whole message must reach the composer, got: {rendered:?}"
    );
}

/// Every frontend renders into the same PTY, so "is it still working" is read
/// from the panel and must behave identically for all three. A frontend-specific
/// activity signal would leave the other two abandoning turns that were fine.
#[test]
fn panel_activity_is_detected_the_same_way_for_every_frontend() {
    for agent_kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&temporary, &cli, agent_kind);
        let live = live_panel(app.command_context.workspace.root());
        app.brain = Some(panel_controller(&app, live));
        let start = std::time::Instant::now();
        // The turn has been open far longer than the deadline, so only the
        // panel decides whether it is still working.
        let started = start
            .checked_sub(std::time::Duration::from_secs(3600))
            .expect("a turn opened an hour ago");
        let actor = app.interactive_actor.clone();
        let job = receiver_job(&app, actor, Channel::Sms, "slow request");
        begin_receiver_turn(&mut app, &job, "slow-response", started);

        app.sample_panel_activity(start);
        let baseline = app
            .last_panel_change()
            .unwrap_or_else(|| panic!("{agent_kind:?} recorded no baseline"));

        // Sampling an unchanged screen must not look like fresh work, or a
        // wedged turn would be waited on forever.
        app.sample_panel_activity(start + std::time::Duration::from_secs(4));
        assert_eq!(
            app.last_panel_change(),
            Some(baseline),
            "{agent_kind:?} treated a static panel as activity"
        );

        // The agent renders something: that is work in progress.
        app.brain
            .as_mut()
            .expect("panel")
            .type_text("working")
            .expect("render into the panel");
        assert!(
            wait_for_panel_contents(app.brain.as_ref().expect("panel"), "working"),
            "{agent_kind:?} panel never echoed"
        );
        let later = start + std::time::Duration::from_secs(8);
        app.sample_panel_activity(later);
        assert_eq!(
            app.last_panel_change(),
            Some(later),
            "{agent_kind:?} missed visible work"
        );

        // Long past the deadline, but the panel moved a moment ago: this turn
        // is slow, not stalled, and must be left to finish.
        assert!(
            !crate::tui::receiver_state::abandons_stalled_turn(
                app.receiver.remote_started_at(),
                app.last_panel_change(),
                later + std::time::Duration::from_secs(10),
            ),
            "{agent_kind:?} abandoned a turn that was still working"
        );
        assert!(
            crate::tui::receiver_state::abandons_stalled_turn(
                app.receiver.remote_started_at(),
                app.last_panel_change(),
                later + crate::tui::receiver_state::ACTIVE_WORK_IDLE,
            ),
            "{agent_kind:?} never gave up on a panel that went quiet"
        );
    }
}

/// One inbound job, so a control-command test reads as the message it is.
fn sms_job(app: &App, actor: &crate::actor::ActorContext, prompt: &str) -> InboundJob {
    InboundJob {
        job_id: uuid::Uuid::new_v4(),
        workspace_id: app.command_context.workspace.id(),
        actor: actor.clone(),
        channel: Channel::Sms,
        prompt: prompt.to_owned(),
        authenticated_sender: "+15551234567".to_owned(),
        attachments: Vec::new(),
        received_at_unix_ms: 1,
        provider_id: Some("provider-message-1".to_owned()),
        thread_participants: vec!["+15551234567".to_owned()],
        response_email: None,
        allowed_response_recipients: Vec::new(),
        email_reply: None,
    }
}

/// A restart is how a sender escapes a backlog, so it must clear that backlog
/// rather than join it, and none of it may reach the agent as a prompt.
#[test]
fn a_restart_command_clears_the_backlog_and_is_never_sent_to_the_agent() {
    let cli = Cli::parse_from(["tasks"]);
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    let actor = sms_actor();
    let recording = LaunchRecording::default();
    app.brain_transport_override = Some(Box::new(LaunchRecordingTransport {
        recording: recording.clone(),
        alive: false,
    }));
    for prompt in [
        "stuck one",
        "stuck two",
        "/ReStArT",
        "sent after the restart",
    ] {
        let job = sms_job(&app, &actor, prompt);
        enqueue_receiver_job(&mut app, job);
    }

    app.tick_receiver();

    assert!(
        !app.receiver.has_pending_work(),
        "the survivor should have been dispatched, not left waiting"
    );
    let specs = recording.0.lock().unwrap();
    assert_eq!(specs.len(), 1, "exactly one message survived the restart");
    assert!(
        specs[0].command.contains("sent after the restart"),
        "the survivor is the message sent after the restart: {}",
        specs[0].command
    );
    for launched in specs.iter() {
        for dropped in ["/ReStArT", "stuck one", "stuck two"] {
            assert!(
                !launched.command.contains(dropped),
                "{dropped:?} was dropped or obeyed and must not reach the agent as a prompt"
            );
        }
    }
}

/// `/new` retires the channel's conversation: the message after it must open a
/// fresh session instead of resuming the one the sender asked to leave.
#[test]
fn a_new_command_retires_the_channel_session_without_becoming_a_prompt() {
    let cli = Cli::parse_from(["tasks"]);
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.config.access_mode = crate::access::AccessMode::WorkspaceOnly;
    let actor = sms_actor();
    let scope = SessionScope::new(
        AgentKind::Claude,
        app.command_context.workspace.id(),
        actor.clone(),
    );
    // A session that would otherwise be resumed: registered, released, and
    // backed by a transcript that really exists on disk.
    let session = AgentSession::new("previous-sms-conversation").unwrap();
    SessionStore::register(&app.db, &session, "prior-shell", 42, &scope).unwrap();
    SessionStore::release(&app.db, "prior-shell").unwrap();
    let _transcript = ClaudeTranscript::create(
        app.command_context.workspace.root(),
        "previous-sms-conversation",
    );
    let recording = LaunchRecording::default();
    app.brain_transport_override = Some(Box::new(LaunchRecordingTransport {
        recording: recording.clone(),
        alive: false,
    }));
    for prompt in ["/NEW", "what is on today?"] {
        let job = sms_job(&app, &actor, prompt);
        enqueue_receiver_job(&mut app, job);
    }

    app.tick_receiver();

    let specs = recording.0.lock().unwrap();
    assert_eq!(specs.len(), 1, "only the real message launches anything");
    let command = &specs[0].command;
    assert!(
        command.contains("what is on today?"),
        "the message after the command is what gets asked: {command}"
    );
    assert!(
        !command.contains("/NEW") && !command.contains("/new'"),
        "the command itself must not be answered as a prompt: {command}"
    );
    assert!(
        !command.contains("--resume"),
        "the retired conversation must not be resumed: {command}"
    );
    drop(specs);
    assert!(
        !app.receiver.has_pending_channel_reset(),
        "the fresh-session request is consumed by the launch it applies to"
    );
}
