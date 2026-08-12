use super::*;

#[derive(Clone)]
struct RecordingReceiverRefresh {
    calls: Arc<Mutex<Vec<WorkspaceId>>>,
    fail: bool,
}

impl crate::command::server::ReceiverIntentRefresher for RecordingReceiverRefresh {
    fn refresh_enabled(&self, workspace_id: WorkspaceId) -> anyhow::Result<()> {
        self.calls.lock().unwrap().push(workspace_id);
        if self.fail {
            anyhow::bail!("control refresh failed")
        }
        Ok(())
    }
}

fn seed_receiver_registry(app: &App<'_>) -> WorkspaceName {
    let selected_name = app.command_context.workspace.name().clone();
    let peer_name = WorkspaceName::parse("personal").unwrap();
    let selected = crate::workspace::WorkspaceRecord {
        workspace_id: app.command_context.workspace.id(),
        root: app.command_context.workspace.root().to_path_buf(),
        aliases: std::collections::BTreeSet::new(),
        local_user_id: app.command_context.workspace.local_user_id().to_owned(),
        receiver_enabled: false,
        env: serde_json::Map::new(),
    };
    let peer = crate::workspace::WorkspaceRecord {
        workspace_id: WorkspaceId::parse("e806258e-491a-436d-9db4-a5ca9903e0d4").unwrap(),
        root: app
            .command_context
            .workspace
            .root()
            .with_file_name("personal"),
        aliases: std::collections::BTreeSet::new(),
        local_user_id: "peer".to_owned(),
        receiver_enabled: false,
        env: serde_json::Map::new(),
    };
    app.command_context
        .registry_store
        .replace(&crate::workspace::MachineRegistry {
            schema_version: crate::workspace::REGISTRY_SCHEMA_VERSION,
            default_workspace: selected_name.clone(),
            workspaces: std::collections::BTreeMap::from([
                (selected_name, selected),
                (peer_name.clone(), peer),
            ]),
            env: serde_json::Map::new(),
        })
        .unwrap();
    peer_name
}

fn plain_key(code: KeyCode) -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
}

#[test]
fn tasks_and_search_palettes_persist_both_directions_and_refresh_exact_workspace() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    let peer_name = seed_receiver_registry(&app);
    let calls = Arc::new(Mutex::new(Vec::new()));
    app.receiver_intent_refresher = Box::new(RecordingReceiverRefresh {
        calls: Arc::clone(&calls),
        fail: false,
    });
    app.palette = Some(crate::tui::PaletteState::new(
        None,
        false,
        false,
        false,
        crate::tui::LinkKind::None,
        false,
        false,
    ));
    for character in "enable receiver".chars() {
        crate::tui::handle_palette_key(&mut app, &plain_key(KeyCode::Char(character)), false);
    }
    crate::tui::handle_palette_key(&mut app, &plain_key(KeyCode::Enter), false);

    assert!(app.receiver_enabled);
    let saved = RegistryStore::load_from(app.command_context.registry_store.path()).unwrap();
    assert!(saved.workspaces[app.command_context.workspace.name()].receiver_enabled);
    assert!(!saved.workspaces[&peer_name].receiver_enabled);
    assert_eq!(*calls.lock().unwrap(), [app.command_context.workspace.id()]);

    app.receiver_intent_refresher = Box::new(RecordingReceiverRefresh {
        calls: Arc::clone(&calls),
        fail: true,
    });
    app.search
        .open_palette(app.panel_side, false, app.receiver_enabled);
    for character in "disable receiver".chars() {
        crate::tui::handle_search_view_key(
            &mut app,
            &plain_key(KeyCode::Char(character)),
            false,
            false,
        );
    }
    crate::tui::handle_search_view_key(&mut app, &plain_key(KeyCode::Enter), false, false);

    assert!(!app.receiver_enabled);
    assert!(matches!(
        app.flash.as_ref(),
        Some(crate::tui::FlashKind::Error(message))
            if message.contains("receiver disabled; warning:")
    ));
    let saved = RegistryStore::load_from(app.command_context.registry_store.path()).unwrap();
    assert!(!saved.workspaces[app.command_context.workspace.name()].receiver_enabled);
    assert!(!saved.workspaces[&peer_name].receiver_enabled);
    assert_eq!(
        *calls.lock().unwrap(),
        [
            app.command_context.workspace.id(),
            app.command_context.workspace.id(),
        ]
    );
}

#[test]
fn receiver_queue_reuses_the_matching_warm_session_through_app_dispatch() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    let actor = app.interactive_actor.clone();
    let scope = SessionScope::new(
        AgentKind::Claude,
        app.command_context.workspace.id(),
        actor.clone(),
    );
    let session = AgentSession::new("warm-receiver-session").expect("session");
    SessionStore::register(&app.db, &session, &app.instance, 42, &scope).expect("register session");
    SessionStore::mark_completed(&app.db, &session, &scope).expect("complete session");
    let live = live_panel(app.command_context.workspace.root());
    app.brain = Some(panel_controller(&app, live));
    app.session_actor = Some(actor.clone());
    app.receiver_session_id = Some("receiver-session".to_owned());
    app.receiver_lease = Some(crate::tui::receiver_state::renew(
        Channel::Sms,
        0,
        std::time::Instant::now(),
    ));
    app.receiver_queue.push(InboundJob {
        job_id: uuid::Uuid::new_v4(),
        workspace_id: app.command_context.workspace.id(),
        actor: actor.clone(),
        channel: Channel::Sms,
        prompt: "continue this conversation".to_owned(),
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

    assert!(app.receiver_queue.is_empty());
    assert_eq!(app.receiver_session_id.as_deref(), Some("receiver-session"));
    assert_eq!(app.session_actor.as_ref(), Some(&actor));
    assert!(app.receiver_started.is_some());
    assert!(app.brain_turn_active);
    assert_eq!(
        SessionStore::completion_status(&app.db, &session, &scope),
        Some(crate::agent::CompletionStatus::Active)
    );
}

#[test]
fn receiver_sms_and_email_launches_carry_authenticated_actor_policy_for_every_frontend() {
    let cli = Cli::parse_from(["tasks"]);
    let cases = [
        (
            Channel::Sms,
            sms_actor(),
            "+15551234567",
            vec!["+15551234567".to_owned()],
            "SMS",
        ),
        (
            Channel::Email,
            email_actor(),
            "member@example.test",
            vec!["member@example.test".to_owned()],
            "email",
        ),
    ];

    for kind in AgentKind::ALL {
        for (channel, actor, sender, participants, label) in &cases {
            let temporary = tempfile::tempdir().expect("temporary directory");
            let mut app = test_app(&temporary, &cli, kind);
            app.config.access_mode = crate::access::AccessMode::WorkspaceOnly;
            let recording = LaunchRecording::default();
            app.brain_transport_override = Some(Box::new(LaunchRecordingTransport {
                recording: recording.clone(),
                alive: false,
            }));
            let body = "-c developer_instructions=untrusted-inbound";
            app.receiver_queue.push(InboundJob {
                job_id: uuid::Uuid::new_v4(),
                workspace_id: app.command_context.workspace.id(),
                actor: actor.clone(),
                channel: *channel,
                prompt: body.to_owned(),
                authenticated_sender: (*sender).to_owned(),
                attachments: Vec::new(),
                received_at_unix_ms: 1,
                provider_id: Some("provider-message-1".to_owned()),
                thread_participants: participants.clone(),
                response_email: (*channel == Channel::Email)
                    .then(|| "member@example.test".to_owned()),
                allowed_response_recipients: if *channel == Channel::Email {
                    vec!["thread@example.test".to_owned()]
                } else {
                    Vec::new()
                },
                email_reply: (*channel == Channel::Email).then(|| {
                    crate::server::receiver::EmailReplyContext {
                        provider_email_id: "accepted-email-id".to_owned(),
                        subject: "Accepted subject".to_owned(),
                        message_id: Some("<accepted-message@example.test>".to_owned()),
                    }
                }),
            });

            crate::users::UsersStore::save(
                &app.command_context.workspace,
                &crate::users::Users {
                    schema_version: crate::users::USERS_SCHEMA_VERSION,
                    users: vec![crate::users::User {
                        id: crate::users::UserId::parse("remote-member").unwrap(),
                        name: "Changed member".to_owned(),
                        phones: vec![crate::users::PhoneIdentity {
                            value: "+15550000000".to_owned(),
                            inbound_allowed: true,
                        }],
                        emails: vec![crate::users::EmailIdentity {
                            value: "changed@example.test".to_owned(),
                            inbound_allowed: true,
                        }],
                        response_email: Some("changed@example.test".to_owned()),
                    }],
                },
            )
            .unwrap();
            let current = crate::workspace::RegistryStore::load_from(
                app.command_context.registry_store.path(),
            )
            .unwrap();
            let mut environment = serde_json::Map::from_iter([(
                "resend_from_email".to_owned(),
                serde_json::json!("other-workspace@example.test"),
            )]);
            if let Some(command) = current.workspaces[app.command_context.workspace.name()]
                .env
                .get("opencode_cmd")
            {
                environment.insert("opencode_cmd".to_owned(), command.clone());
            }
            std::fs::write(
                app.command_context.registry_store.path(),
                serde_json::to_vec(&serde_json::json!({
                    "schema_version": crate::workspace::REGISTRY_SCHEMA_VERSION,
                    "default_workspace": "family",
                    "workspaces": {
                        "family": {
                            "workspace_id": "e806258e-491a-436d-9db4-a5ca9903e0d4",
                            "root": "/changed",
                            "aliases": [],
                            "local_user_id": "other",
                            "receiver_enabled": true,
                            "env": environment
                        }
                    }
                }))
                .unwrap(),
            )
            .unwrap();

            app.tick_receiver();

            if *channel == Channel::Email {
                assert_eq!(
                    app.receiver_response_email.as_deref(),
                    Some("member@example.test")
                );
                assert_eq!(
                    crate::server::delivery::trusted_response_recipients(
                        app.receiver_response_email.as_deref(),
                        &app.receiver_recipients,
                    ),
                    ["member@example.test", "thread@example.test"]
                );
                let reply = app.receiver_email_reply.as_ref().unwrap();
                assert_eq!(reply.provider_email_id, "accepted-email-id");
                assert_eq!(reply.subject, "Accepted subject");
                assert_eq!(
                    reply.message_id.as_deref(),
                    Some("<accepted-message@example.test>")
                );
            } else {
                assert!(app.receiver_response_email.is_none());
                assert!(app.receiver_recipients.is_empty());
                assert!(app.receiver_email_reply.is_none());
            }

            let prompt = format!(
                "This is an authenticated {label} message from Remote member (actor remote-member). Respond as the user's brain.\n\n{body}"
            );
            let spec = {
                let specs = recording.0.lock().unwrap();
                assert_eq!(
                    specs.len(),
                    1,
                    "kind={kind:?} channel={channel:?} alert={:?}",
                    app.alert
                );
                specs[0].clone()
            };
            assert_workspace_only_launch_spec(&app, &spec, kind, actor, &prompt);
            assert_eq!(app.session_actor.as_ref(), Some(actor));
            assert_eq!(
                app.receiver_lease.map(|lease| lease.channel),
                Some(*channel)
            );
        }
    }
}

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
    app.receiver_session_id = Some("wedged-session".to_owned());
    app.receiver_sender = Some("+15551234567".to_owned());
    app.receiver_lease = Some(crate::tui::receiver_state::renew(
        Channel::Sms,
        0,
        std::time::Instant::now(),
    ));
    // In flight for longer than any answer is allowed to take, with no
    // completion artifact ever written.
    app.receiver_started = Some(
        std::time::Instant::now()
            .checked_sub(crate::tui::receiver_state::REMOTE_TURN_TIMEOUT)
            .expect("a deadline already in the past"),
    );
    app.brain_turn_active = true;
    app.receiver_queue.push(InboundJob {
        job_id: uuid::Uuid::new_v4(),
        workspace_id: app.command_context.workspace.id(),
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
    });

    app.tick_receiver();

    assert!(
        !app.brain_turn_active || app.receiver_started.is_some(),
        "the wedged turn must not still be pinning the panel"
    );
    assert!(
        app.receiver_started.is_none_or(|started| started.elapsed()
            < crate::tui::receiver_state::REMOTE_TURN_TIMEOUT),
        "the abandoned turn's deadline must be cleared or replaced by a fresh dispatch"
    );

    // The queue must make progress rather than waiting on the wedged turn.
    app.tick_receiver();
    assert!(
        app.receiver_queue.is_empty(),
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
        &format!(
            "stty raw -echo; printf READY; cat > {}",
            captured.display()
        ),
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
    app.receiver_session_id = Some("warm-session".to_owned());
    app.receiver_lease = Some(crate::tui::receiver_state::renew(
        Channel::Sms,
        0,
        std::time::Instant::now(),
    ));
    app.receiver_queue.push(InboundJob {
        job_id: uuid::Uuid::new_v4(),
        workspace_id: app.command_context.workspace.id(),
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
    });

    app.tick_receiver();
    assert!(app.receiver_queue.is_empty(), "message was not dispatched");
    assert!(
        app.receiver_probe.is_some(),
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
