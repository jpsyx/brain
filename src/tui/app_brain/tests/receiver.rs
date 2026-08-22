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

fn seed_receiver_registry(app: &App) -> WorkspaceName {
    let selected_name = app.context.workspace().name().clone();
    let peer_name = WorkspaceName::parse("personal").unwrap();
    let selected = crate::workspace::WorkspaceRecord {
        workspace_id: app.context.workspace().id(),
        root: app.context.workspace().root().to_path_buf(),
        aliases: std::collections::BTreeSet::new(),
        local_user_id: app.context.workspace().local_user_id().to_owned(),
        receiver_enabled: false,
        env: serde_json::Map::new(),
    };
    let peer = crate::workspace::WorkspaceRecord {
        workspace_id: WorkspaceId::parse("e806258e-491a-436d-9db4-a5ca9903e0d4").unwrap(),
        root: app.context.workspace().root().with_file_name("personal"),
        aliases: std::collections::BTreeSet::new(),
        local_user_id: "peer".to_owned(),
        receiver_enabled: false,
        env: serde_json::Map::new(),
    };
    app.context
        .command()
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
    app.receiver
        .replace_intent_refresher(Box::new(RecordingReceiverRefresh {
            calls: Arc::clone(&calls),
            fail: false,
        }));
    app.overlay = Some(crate::tui::Overlay::TaskPalette(
        crate::tui::TaskPalette::new(
            None,
            false,
            false,
            false,
            crate::tui::LinkKind::None,
            false,
            false,
        ),
    ));
    for character in "enable receiver".chars() {
        crate::tui::handle_palette_key(&mut app, &plain_key(KeyCode::Char(character)), false);
    }
    crate::tui::handle_palette_key(&mut app, &plain_key(KeyCode::Enter), false);

    assert!(app.receiver.is_enabled());
    let saved = RegistryStore::load_from(app.context.command().registry_store.path()).unwrap();
    assert!(saved.workspaces[app.context.workspace().name()].receiver_enabled);
    assert!(!saved.workspaces[&peer_name].receiver_enabled);
    assert_eq!(*calls.lock().unwrap(), [app.context.workspace().id()]);

    app.receiver
        .replace_intent_refresher(Box::new(RecordingReceiverRefresh {
            calls: Arc::clone(&calls),
            fail: true,
        }));
    app.overlay = Some(crate::tui::Overlay::SearchPalette(
        app.shell.search_palette(false, app.receiver.is_enabled()),
    ));
    for character in "disable receiver".chars() {
        crate::tui::route_search_palette(&mut app, &plain_key(KeyCode::Char(character)));
    }
    crate::tui::route_search_palette(&mut app, &plain_key(KeyCode::Enter));

    assert!(!app.receiver.is_enabled());
    assert!(matches!(
        app.status.flash(),
        Some(crate::tui::FlashKind::Error(message))
            if message.contains("receiver disabled; warning:")
    ));
    let saved = RegistryStore::load_from(app.context.command().registry_store.path()).unwrap();
    assert!(!saved.workspaces[app.context.workspace().name()].receiver_enabled);
    assert!(!saved.workspaces[&peer_name].receiver_enabled);
    assert_eq!(
        *calls.lock().unwrap(),
        [app.context.workspace().id(), app.context.workspace().id(),]
    );
}

#[test]
fn receiver_queue_reuses_the_matching_warm_session_through_app_dispatch() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    let actor = app.brain.interactive_actor().clone();
    let scope = SessionScope::new(
        AgentKind::Claude,
        app.context.workspace().id(),
        actor.clone(),
    );
    let session = AgentSession::new("warm-receiver-session").expect("session");
    SessionStore::register(&app.services, &session, app.brain.instance(), 42, &scope)
        .expect("register session");
    SessionStore::mark_completed(&app.services, &session, &scope).expect("complete session");
    let live = live_panel(app.context.workspace().root());
    let controller = panel_controller(&app, live);
    app.brain.install_main(controller);
    let warm_job = receiver_job(&app, actor.clone(), Channel::Sms, "previous message");
    warm_receiver_session(
        &mut app,
        &warm_job,
        "receiver-session",
        std::time::Instant::now(),
    );
    let workspace_id = app.context.workspace().id();
    enqueue_receiver_job(
        &mut app,
        InboundJob {
            job_id: uuid::Uuid::new_v4(),
            workspace_id,
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
        },
    );

    app.tick_receiver();

    assert!(!app.receiver.has_pending_work());
    assert_eq!(
        app.receiver.receiver_response_id(),
        Some("receiver-session")
    );
    assert_eq!(app.brain.session_actor(), Some(&actor));
    assert!(app.receiver.remote_turn_in_flight());
    assert!(app.brain.turn_active());
    assert_eq!(
        SessionStore::completion_status(&app.services, &session, &scope),
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
            let mut config = app.context.config().clone();
            config.access_mode = crate::access::AccessMode::WorkspaceOnly;
            app.context = app.context.replacing_config(config);
            let recording = LaunchRecording::default();
            app.brain
                .replace_brain_transport(Box::new(LaunchRecordingTransport {
                    recording: recording.clone(),
                    alive: false,
                }));
            let body = "-c developer_instructions=untrusted-inbound";
            let workspace_id = app.context.workspace().id();
            enqueue_receiver_job(
                &mut app,
                InboundJob {
                    job_id: uuid::Uuid::new_v4(),
                    workspace_id,
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
                },
            );

            crate::users::UsersStore::save(
                app.context.workspace(),
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
                app.context.command().registry_store.path(),
            )
            .unwrap();
            let mut environment = serde_json::Map::from_iter([(
                "resend_from_email".to_owned(),
                serde_json::json!("other-workspace@example.test"),
            )]);
            if let Some(command) = current.workspaces[app.context.workspace().name()]
                .env
                .get("opencode_cmd")
            {
                environment.insert("opencode_cmd".to_owned(), command.clone());
            }
            std::fs::write(
                app.context.command().registry_store.path(),
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

            let target = app.receiver.email_reply_target();
            if *channel == Channel::Email {
                assert_eq!(
                    target.response_email.as_deref(),
                    Some("member@example.test")
                );
                assert_eq!(
                    crate::server::delivery::trusted_response_recipients(
                        target.response_email.as_deref(),
                        &target.recipients,
                    ),
                    ["member@example.test", "thread@example.test"]
                );
                let reply = target.reply.as_ref().unwrap();
                assert_eq!(reply.provider_email_id, "accepted-email-id");
                assert_eq!(reply.subject, "Accepted subject");
                assert_eq!(
                    reply.message_id.as_deref(),
                    Some("<accepted-message@example.test>")
                );
            } else {
                assert!(target.response_email.is_none());
                assert!(target.recipients.is_empty());
                assert!(target.reply.is_none());
            }

            let prompt = format!(
                "This is an authenticated {label} message from Remote member (actor remote-member). Respond as the user's brain. If the message asks to add, create, capture, remember, or track a task, create it in Brain's task system; do not perform the task now unless the sender explicitly asks you to.\n\n{body}"
            );
            let spec = {
                let specs = recording.0.lock().unwrap();
                assert_eq!(
                    specs.len(),
                    1,
                    "kind={kind:?} channel={channel:?} alert={:?}",
                    app.status.alert()
                );
                specs[0].clone()
            };
            assert_workspace_only_launch_spec(&app, &spec, kind, actor, &prompt);
            assert_eq!(app.brain.session_actor(), Some(actor));
            assert_eq!(app.receiver.active_channel(), Some(*channel));
        }
    }
}

mod turns;
